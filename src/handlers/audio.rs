use axum::{
    body::Body,
    extract::{Multipart, Path, State, Query},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Extension, Json, Router,
};
use serde::{Serialize, Deserialize};
use sqlx::FromRow;
use std::sync::Arc;
use tower_cookies::Cookies;
use chrono::{DateTime, Utc};

use crate::auth::validate_token;
use crate::error_page::build_error_response;
use crate::media::{
    delete_file, generate_storage_path, generate_thumbnail_only, generate_thumbnail_path,
    generate_preview_path, generate_thumbnail_and_preview, read_file, save_file, validate_extension, 
    generate_short_id, validate_thumbnail, get_extension, ALLOWED_THUMBNAIL_EXTENSIONS, MediaType,

};
use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

// ─── Constants ─────────────────────────────────────────────────────────────────

/// Column list used in all SELECT queries (keep in sync with AudioItem struct)
const AUDIO_COLUMNS: &str = "id, user_id, title, description, original_filename, stored_path, size_bytes, mime_type, visibility, thumbnail_path, pinned, pin_order, short_id, created_at";

/// Column list for audio_thumbnails queries
const AUDIO_THUMBNAIL_COLUMNS: &str = "id, audio_id, short_id, raw_path, thumbnail_path, preview_path, is_primary, sort_order, status, created_at";

// ─── Data Structures ───────────────────────────────────────────────────────────

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct AudioItem {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub original_filename: String,
    pub stored_path: String,
    pub size_bytes: i64,
    pub mime_type: String,
    pub visibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    pub pinned: bool,
    pub pin_order: i32,
    pub short_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAudioRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderAudioPinsRequest {
    pub ordered_ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CoverThumbnailQuery {
    pub primary: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetPrimaryCoverRequest {
    pub short_id: String,
}

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct AudioThumbnail {
    pub id: i32,
    pub audio_id: i32,
    pub short_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<String>,
    pub is_primary: bool,
    pub sort_order: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

// ─── Routes ────────────────────────────────────────────────────────────────────

/// Public routes (no auth required, but private items check cookie/header)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audio/public", get(list_public_audio))
        .route("/audio/info/{short_id}", get(get_audio_by_short_id))
        .route("/audio/{id}", get(get_audio))
        .route("/audio/d/{id}", get(download_audio))
        .route("/audio/r/{short_id}", get(serve_audio_stream))
        .route("/audio/cover/{short_id_cover}", get(serve_audio_cover_raw))
        .route("/audio/cover/t/{short_id_cover}", get(serve_audio_cover_thumbnail))
        .route("/audio/cover/p/{short_id_cover}", get(serve_audio_cover_preview))
        .route("/audio/{short_id_audio}/cover/{short_id_cover}", get(get_audio_cover_scoped))
}

/// Protected routes (require auth middleware)
pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audio", post(upload_audio))
        .route("/audio/me", get(list_my_audio))
        .route("/audio/{id}", patch(update_audio).delete(delete_audio))
        .route("/audio/me/pinned", get(list_pinned_audio))
        .route("/audio/reorder-pins", patch(reorder_audio_pins))
        .route("/audio/{id}/cover", post(add_audio_thumbnails).get(list_audio_thumbnails).patch(set_primary_audio_thumbnail).delete(delete_audio_thumbnail))
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

/// Extract optional authentication from cookie or Authorization header
fn extract_optional_auth(
    cookies: &Cookies,
    headers: &HeaderMap,
    jwt_secret: &str,
) -> Option<AuthUser> {
    // Priority 1: Cookie
    let from_cookie = cookies
        .get("auth_token")
        .and_then(|c| validate_token(c.value(), jwt_secret).ok());

    if from_cookie.is_some() {
        return from_cookie;
    }

    // Priority 2: Authorization header
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|auth_header| {
            if auth_header.starts_with("Bearer ") {
                validate_token(&auth_header[7..], jwt_secret).ok()
            } else {
                None
            }
        })
}

/// Check if a file extension needs AAC → M4A remux
fn needs_remux(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("aac")
}

/// Remux AAC to M4A container using FFmpeg (lossless, `-c:a copy` only)
async fn ffmpeg_remux_aac_to_m4a(input_path: &str, output_path: &str) -> Result<(), String> {
    let result = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input_path,
            "-c:a", "copy",
            output_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn ffmpeg for remux: {}", e))?;

    if result.success() {
        // Verify the output file was actually created
        if tokio::fs::metadata(output_path).await.is_ok() {
            return Ok(());
        }
    }

    Err("FFmpeg remux failed or produced no output".to_string())
}

// ─── Handler Functions ─────────────────────────────────────────────────────────

// GET /api/audio/me - List audio (superuser sees all, others see only their own) with cursor pagination
async fn list_my_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<AudioItem>>>) {
    let items: Result<Vec<AudioItem>, _> = if auth_user.can_view_all_media() {
        sqlx::query_as(&format!("SELECT {} FROM audio", AUDIO_COLUMNS))
            .fetch_all(&state.db.pool)
            .await
    } else {
        sqlx::query_as(&format!(
            "SELECT {} FROM audio WHERE user_id = ?",
            AUDIO_COLUMNS
        ))
        .bind(auth_user.id)
        .fetch_all(&state.db.pool)
        .await
    };

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch audio items")),
        ),
    }
}

// GET /api/audio/public - List public audio (no auth required)
async fn list_public_audio(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<AudioItem>>>) {
    let items: Result<Vec<AudioItem>, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE visibility = 'public' ORDER BY id DESC",
        AUDIO_COLUMNS
    ))
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch public audio")),
        ),
    }
}

// POST /api/audio - Upload audio (multipart/form-data, no size limit)
async fn upload_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse<AudioItem>>) {
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut visibility: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut original_filename: Option<String> = None;
    let mut thumbnail_data: Option<Vec<u8>> = None;
    let mut thumbnail_filename: Option<String> = None;

    // Parse multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "title" => {
                if let Ok(text) = field.text().await {
                    title = Some(text.trim().to_string());
                }
            }
            "description" => {
                if let Ok(text) = field.text().await {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        description = Some(trimmed);
                    }
                }
            }
            "visibility" => {
                if let Ok(text) = field.text().await {
                    let val = text.trim().to_lowercase();
                    if val == "public" || val == "private" {
                        visibility = Some(val);
                    }
                }
            }
            "file" => {
                original_filename = field.file_name().map(|s| s.to_string());
                if let Ok(bytes) = field.bytes().await {
                    file_data = Some(bytes.to_vec());
                }
            }
            "thumbnail" => {
                // Optional cover art image; ignore if empty or unreadable
                let filename = field.file_name().map(|s| s.to_string());
                if let Ok(bytes) = field.bytes().await {
                    if !bytes.is_empty() {
                        thumbnail_data = Some(bytes.to_vec());
                        thumbnail_filename = filename;
                    }
                }
            }
            _ => {}
        }
    }

    // Validate required fields
    let file_bytes = match file_data {
        Some(data) if !data.is_empty() => data,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("No file provided")),
            );
        }
    };

    let orig_filename = match original_filename {
        Some(name) if !name.is_empty() => name,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("File must have a filename")),
            );
        }
    };

    let title = title.unwrap_or_else(|| orig_filename.clone());
    let visibility = visibility.unwrap_or_else(|| "private".to_string());

    // Validate extension
    let extension = match validate_extension(MediaType::Audio, &orig_filename) {
        Ok(ext) => ext,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(msg)));
        }
    };

    // Original size (before any processing)
    let size_bytes = file_bytes.len() as i64;

    // Determine target extension and generate paths
    let target_extension = if needs_remux(&extension) {
        "m4a".to_string()
    } else {
        extension.clone()
    };

    let (stored_path, full_path) =
        generate_storage_path(&state.config.storage_dir, MediaType::Audio, &target_extension);

    // If remuxing is needed, save to temp file first
    if needs_remux(&extension) {
        let temp_path_str = format!("{}.tmp.aac", full_path.display());
        let temp_path_buf = std::path::PathBuf::from(&temp_path_str);

        // Save the raw AAC file to temp
        if let Err(e) = save_file(&temp_path_buf, &file_bytes).await {
            tracing::error!("Failed to save temp AAC file: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save file")),
            );
        }

        // Remux to M4A
        if let Err(e) = ffmpeg_remux_aac_to_m4a(temp_path_str.as_str(), full_path.to_str().unwrap_or("")).await {
            tracing::error!("AAC remux failed: {}", e);
            // Clean up temp file
            let _ = tokio::fs::remove_file(&temp_path_str).await;
            // Clean up output file if it was partially created
            let _ = tokio::fs::remove_file(&full_path).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to process audio file")),
            );
        }

        // Clean up temp file
        if let Err(e) = tokio::fs::remove_file(&temp_path_str).await {
            tracing::warn!("Failed to clean up temp AAC file: {}", e);
        }
    } else {
        // Save file as-is
        if let Err(e) = save_file(&full_path, &file_bytes).await {
            tracing::error!("Failed to save file: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save file")),
            );
        }
    }

    // Get MIME type from final stored extension
    let mime_type = MediaType::Audio.mime_type_for_extension(&target_extension);

    // Generate short_id with collision retry
    let short_id = loop {
        let candidate = generate_short_id();
        let exists: Result<Option<(i32,)>, _> =
            sqlx::query_as("SELECT id FROM audio WHERE short_id = ?")
                .bind(&candidate)
                .fetch_optional(&state.db.pool)
                .await;
        match exists {
            Ok(None) => break candidate,
            Ok(Some(_)) => continue,
            Err(e) => {
                tracing::error!("Failed to check short_id collision: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to generate unique short_id")),
                );
            }
        }
    };

    // Process optional thumbnail (cover art). Failure here is non-fatal — the audio
    // upload still succeeds, just without a thumbnail.
    let mut thumbnail_path: Option<String> = None;
    if let (Some(thumb_bytes), Some(thumb_filename)) = (thumbnail_data, thumbnail_filename) {
        if let Err(msg) = validate_thumbnail(&thumb_filename, thumb_bytes.len()) {
            tracing::warn!("Skipping audio thumbnail: {}", msg);
        } else {
            let thumb_ext = thumb_filename
                .rsplit('.')
                .next()
                .map(|e| format!(".{}", e.to_lowercase()));

            if let Some(ext) = &thumb_ext {
                if ALLOWED_THUMBNAIL_EXTENSIONS.contains(&ext.as_str()) {
                    let generated_thumb_path = generate_thumbnail_path(&stored_path);
                    let thumb_full_path = std::path::PathBuf::from(&state.config.storage_dir)
                        .join(&generated_thumb_path);

                    let permit = state.image_semaphore.clone().acquire_owned().await;
                    if let Ok(_permit) = permit {
                        let thumb_result =
                            tokio::task::spawn_blocking(move || generate_thumbnail_only(&thumb_bytes))
                                .await;

                        match thumb_result {
                            Ok(Ok(webp_bytes)) => {
                                if let Err(e) = save_file(&thumb_full_path, &webp_bytes).await {
                                    tracing::warn!("Failed to save audio thumbnail: {}", e);
                                } else {
                                    thumbnail_path = Some(generated_thumb_path);
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("Failed to generate audio thumbnail: {}", e);
                            }
                            Err(e) => {
                                tracing::warn!("Thumbnail generation task panicked: {}", e);
                            }
                        }
                    } else {
                        tracing::warn!("Image semaphore closed, skipping audio thumbnail generation");
                    }
                }
            }
        }
    }

    // Insert into database
    let result = sqlx::query(
        "INSERT INTO audio (user_id, title, description, original_filename, stored_path, size_bytes, mime_type, visibility, thumbnail_path, pinned, pin_order, short_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&title)
    .bind(&description)
    .bind(&orig_filename)
    .bind(&stored_path)
    .bind(size_bytes)
    .bind(mime_type)
    .bind(&visibility)
    .bind(&thumbnail_path)
    .bind(false)  // pinned
    .bind(0)      // pin_order
    .bind(&short_id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = AudioItem {
                id: res.last_insert_id() as i32,
                user_id: auth_user.id,
                title,
                description,
                original_filename: orig_filename,
                stored_path,
                size_bytes,
                mime_type: mime_type.to_string(),
                visibility,
                thumbnail_path,
                pinned: false,
                pin_order: 0,
                short_id,
                created_at: DateTime::from(Utc::now()),
            };
            (StatusCode::CREATED, Json(ApiResponse::success(item)))
        }
        Err(e) => {
            tracing::error!("Failed to insert audio item: {}", e);
            let _ = delete_file(&state.config.storage_dir, &stored_path).await;
            if let Some(tp) = &thumbnail_path {
                let _ = delete_file(&state.config.storage_dir, tp).await;
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save audio metadata")),
            )
        }
    }
}

// GET /api/audio/:id - Get audio metadata (public endpoint with visibility check)
async fn get_audio(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control for private audio
            if item.visibility == "private" {
                let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                match auth_user {
                    Some(user) => {
                        if item.user_id != user.id && !user.is_superuser() {
                            return build_error_response(
                                StatusCode::FORBIDDEN,
                                "You can only access your own private audio",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                    None => {
                        return build_error_response(
                            StatusCode::UNAUTHORIZED,
                            "This audio is private. Authentication required.",
                            &headers,
                            &state.config.frontend_url,
                        );
                    }
                }
            }
            (StatusCode::OK, Json(ApiResponse::success(item))).into_response()
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Audio not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/audio/:id/download - Download the actual audio file (public endpoint with visibility check)
async fn download_audio(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control for private audio
            if item.visibility == "private" {
                let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                match auth_user {
                    Some(user) => {
                        if item.user_id != user.id && !user.is_superuser() {
                            return build_error_response(
                                StatusCode::FORBIDDEN,
                                "You can only access your own private audio",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                    None => {
                        return build_error_response(
                            StatusCode::UNAUTHORIZED,
                            "This audio is private. Authentication required.",
                            &headers,
                            &state.config.frontend_url,
                        );
                    }
                }
            }

            match read_file(&state.config.storage_dir, &item.stored_path).await {
                Ok(data) => {
                    let body = Body::from(data);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, item.mime_type)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", item.original_filename),
                        )
                        .body(body)
                        .unwrap()
                        .into_response()
                }
                Err(_) => build_error_response(
                    StatusCode::NOT_FOUND,
                    "File not found on disk",
                    &headers,
                    &state.config.frontend_url,
                ),
            }
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Audio not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/audio/:id/thumbnail - Serve the audio cover art thumbnail (public endpoint with visibility check)
// PATCH /api/audio/:id - Update audio (owner or superuser) - supports partial updates of title, description, visibility, and pinned
async fn update_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateAudioRequest>,
) -> (StatusCode, Json<ApiResponse<AudioItem>>) {
    const MAX_PINNED_AUDIO: i64 = 8;

    // Reject if all fields are None
    if payload.title.is_none()
        && payload.description.is_none()
        && payload.visibility.is_none()
        && payload.pinned.is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("At least one field must be provided")),
        );
    }

    // Fetch current item
    let mut item: AudioItem = match sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Audio not found")),
            );
        }
    };

    // Ownership check
    if item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only update your own audio")),
        );
    }

    // Apply title update
    if let Some(new_title) = payload.title {
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Title cannot be empty")),
            );
        }
        item.title = trimmed.to_string();
    }

    // Apply description update
    if let Some(new_description) = payload.description {
        let trimmed = new_description.trim();
        item.description = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }

    // Apply visibility update
    if let Some(new_visibility) = payload.visibility {
        let trimmed_lower = new_visibility.trim().to_lowercase();
        if trimmed_lower == "public" || trimmed_lower == "private" {
            item.visibility = trimmed_lower;
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error(
                    "Visibility must be 'public' or 'private'",
                )),
            );
        }
    }

    // Apply pinned update
    if let Some(should_pin) = payload.pinned {
        if should_pin && !item.pinned {
            // Pinning: check limit and assign pin_order
            let pinned_count: Result<(i64,), _> =
                sqlx::query_as("SELECT COUNT(*) FROM audio WHERE user_id = ? AND pinned = TRUE")
                    .bind(auth_user.id)
                    .fetch_one(&state.db.pool)
                    .await;

            match pinned_count {
                Ok((count,)) if count >= MAX_PINNED_AUDIO => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error("You can only pin up to 8 audio items")),
                    );
                }
                Ok(_) => {
                    // Get max pin_order and increment
                    let max_order: Result<(Option<i32>,), _> =
                        sqlx::query_as("SELECT MAX(pin_order) FROM audio WHERE user_id = ? AND pinned = TRUE")
                            .bind(auth_user.id)
                            .fetch_one(&state.db.pool)
                            .await;

                    let new_order = match max_order {
                        Ok((Some(max_val),)) => max_val + 1,
                        _ => 1,
                    };
                    item.pin_order = new_order;
                    item.pinned = true;
                }
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to check pin limit")),
                    );
                }
            }
        } else if !should_pin && item.pinned {
            // Unpinning: reset pin_order
            item.pinned = false;
            item.pin_order = 0;
        }
        // If requested value equals current value, no change
    }

    // Update database
    let result = sqlx::query(
        "UPDATE audio SET title = ?, description = ?, visibility = ?, pinned = ?, pin_order = ? WHERE id = ?",
    )
    .bind(&item.title)
    .bind(&item.description)
    .bind(&item.visibility)
    .bind(item.pinned)
    .bind(item.pin_order)
    .bind(id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(_) => (StatusCode::OK, Json(ApiResponse::success(item))),
        Err(e) => {
            tracing::error!("Failed to update audio: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to update audio")),
            )
        }
    }
}

// GET /api/audio/me/pinned - List current user's pinned audio (protected route)
async fn list_pinned_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<AudioItem>>>) {
    let items: Result<Vec<AudioItem>, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE user_id = ? AND pinned = TRUE ORDER BY pin_order ASC, updated_at DESC",
        AUDIO_COLUMNS
    ))
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch pinned audio")),
        ),
    }
}

// PATCH /api/audio/reorder-pins - Reorder pinned audio items (protected route)
async fn reorder_audio_pins(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReorderAudioPinsRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    const MAX_PINNED_AUDIO: usize = 8;

    // Validate ordered_ids
    if payload.ordered_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("ordered_ids cannot be empty")),
        );
    }

    if payload.ordered_ids.len() > MAX_PINNED_AUDIO {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Cannot reorder more than 8 pinned audio items")),
        );
    }

    // Start transaction
    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to reorder pins")),
            );
        }
    };

    // Validate each item
    for (idx, id) in payload.ordered_ids.iter().enumerate() {
        let item: Result<(i32, bool), _> =
            sqlx::query_as("SELECT user_id, pinned FROM audio WHERE id = ?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await;

        match item {
            Ok((user_id, pinned)) => {
                // Check ownership
                if user_id != auth_user.id && !auth_user.is_superuser() {
                    let _ = tx.rollback().await;
                    return (
                        StatusCode::FORBIDDEN,
                        Json(ApiResponse::error("You can only reorder your own audio")),
                    );
                }
                // Check pinned status
                if !pinned {
                    let _ = tx.rollback().await;
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error(
                            "All items must be pinned to reorder",
                        )),
                    );
                }
            }
            Err(_) => {
                let _ = tx.rollback().await;
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error("Audio item not found")),
                );
            }
        }

        // Update pin_order (1-based)
        let pin_order = (idx + 1) as i32;
        if let Err(e) = sqlx::query("UPDATE audio SET pin_order = ? WHERE id = ?")
            .bind(pin_order)
            .bind(id)
            .execute(&mut *tx)
            .await
        {
            tracing::error!("Failed to update pin_order: {}", e);
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to reorder pins")),
            );
        }
    }

    // Commit transaction
    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to reorder pins")),
        );
    }

    (
        StatusCode::OK,
        Json(ApiResponse::success("Pins reordered successfully".to_string())),
    )
}

// GET /api/audio/info/:short_id - Get audio metadata by short_id (public with visibility check)
async fn get_audio_by_short_id(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    headers: HeaderMap,
    Path(short_id): Path<String>,
) -> impl IntoResponse {
    let item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE short_id = ?",
        AUDIO_COLUMNS
    ))
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control for private audio
            if item.visibility == "private" {
                let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                match auth_user {
                    Some(user) => {
                        if item.user_id != user.id && !user.is_superuser() {
                            return build_error_response(
                                StatusCode::FORBIDDEN,
                                "You can only access your own private audio",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                    None => {
                        return build_error_response(
                            StatusCode::UNAUTHORIZED,
                            "This audio is private. Authentication required.",
                            &headers,
                            &state.config.frontend_url,
                        );
                    }
                }
            }
            (StatusCode::OK, Json(ApiResponse::success(item))).into_response()
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Audio not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// DELETE /api/audio/:id - Delete audio (owner or superuser)
async fn delete_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only delete your own audio")),
                );
            }

            // Fetch all thumbnails (with all paths) before deleting audio (for cascading delete)
            let thumbnails: Result<Vec<(Option<String>, Option<String>, Option<String>)>, _> =
                sqlx::query_as("SELECT raw_path, thumbnail_path, preview_path FROM audio_thumbnails WHERE audio_id = ?")
                    .bind(id)
                    .fetch_all(&state.db.pool)
                    .await;

            // Delete audio (CASCADE deletes audio_thumbnails rows automatically)
            let result = sqlx::query("DELETE FROM audio WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    // Delete main audio file from disk
                    if let Err(e) = delete_file(&state.config.storage_dir, &item.stored_path).await {
                        tracing::warn!("Failed to delete audio file from disk: {}", e);
                    }

                    // Delete all thumbnail files (raw, thumbnail, preview)
                    if let Ok(thumbs) = thumbnails {
                        for (raw_path, thumbnail_path, preview_path) in thumbs {
                            if let Some(path) = raw_path {
                                if let Err(e) = delete_file(&state.config.storage_dir, &path).await {
                                    tracing::warn!("Failed to delete cover raw file from disk: {}", e);
                                }
                            }
                            if let Some(path) = thumbnail_path {
                                if let Err(e) = delete_file(&state.config.storage_dir, &path).await {
                                    tracing::warn!("Failed to delete cover thumbnail file from disk: {}", e);
                                }
                            }
                            if let Some(path) = preview_path {
                                if let Err(e) = delete_file(&state.config.storage_dir, &path).await {
                                    tracing::warn!("Failed to delete cover preview file from disk: {}", e);
                                }
                            }
                        }
                    }

                    (
                        StatusCode::OK,
                        Json(ApiResponse::success("Audio deleted".to_string())),
                    )
                }
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to delete audio")),
                ),
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Audio not found")),
        ),
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// NEW AUDIO THUMBNAIL ENDPOINTS (Part 2)
// ───────────────────────────────────────────────────────────────────────────────

// POST /api/audio/:id/thumbnails - Add up to 20 thumbnails to an audio item (async two-phase processing)
async fn add_audio_thumbnails(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse<Vec<AudioThumbnail>>>) {
    const MAX_THUMBNAILS_PER_AUDIO: i64 = 20;

    // Fetch audio item to verify ownership
    let audio_item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let audio_item = match audio_item {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Audio not found")),
            );
        }
    };

    // Ownership check
    if audio_item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only add thumbnails to your own audio")),
        );
    }

    // Check current thumbnail count
    let current_count: Result<(i64,), _> =
        sqlx::query_as("SELECT COUNT(*) FROM audio_thumbnails WHERE audio_id = ?")
            .bind(id)
            .fetch_one(&state.db.pool)
            .await;

    let current_count = match current_count {
        Ok((count,)) => count,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to check thumbnail limit")),
            );
        }
    };

    if current_count >= MAX_THUMBNAILS_PER_AUDIO {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Maximum 20 thumbnails per audio item")),
        );
    }

    let mut raw_files = Vec::new();

    // PHASE 1: Parse multipart, save raw files, and insert DB rows with status='processing'
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "thumbnails" {
            let filename = field.file_name().unwrap_or("thumbnail.jpg").to_string();

            if let Ok(bytes) = field.bytes().await {
                if bytes.is_empty() {
                    continue;
                }

                // Validate filename and size
                if let Err(msg) = validate_thumbnail(&filename, bytes.len()) {
                    tracing::warn!("Skipping thumbnail: {}", msg);
                    continue;
                }

                // Stop if we've reached the limit while processing
                if raw_files.len() as i64 >= (MAX_THUMBNAILS_PER_AUDIO - current_count) {
                    break;
                }

                raw_files.push((filename, bytes.to_vec()));
            }
        }
    }

    if raw_files.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No valid thumbnails were uploaded")),
        );
    }

    let mut uploaded_thumbnails: Vec<AudioThumbnail> = Vec::new();

    // Save raw files and insert DB rows
    for (filename, file_bytes) in raw_files {
        let ext = get_extension(&filename).unwrap_or_default();
        let (raw_path, full_path) =
            generate_storage_path(&state.config.storage_dir, MediaType::Audio, &ext);

        // Save raw file to disk
        if let Err(e) = save_file(&full_path, &file_bytes).await {
            tracing::error!("Failed to save raw thumbnail file {}: {}", filename, e);
            // Clean up already-saved files on error
            for item in &uploaded_thumbnails {
                let _ = delete_file(&state.config.storage_dir, &item.raw_path.as_ref().unwrap_or(&String::new())).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save thumbnail file to disk")),
            );
        }

        // Generate unique short_id for this cover
        let short_id = loop {
            let candidate = generate_short_id();
            let exists: Result<Option<(i32,)>, _> = sqlx::query_as(
                "SELECT id FROM audio_thumbnails WHERE short_id = ?"
            )
            .bind(&candidate)
            .fetch_optional(&state.db.pool)
            .await;

            match exists {
                Ok(None) => break candidate,
                Ok(Some(_)) => continue,
                Err(e) => {
                    tracing::error!("Failed to check short_id uniqueness: {}", e);
                    for item in &uploaded_thumbnails {
                        let _ = delete_file(&state.config.storage_dir, &item.raw_path.as_ref().unwrap_or(&String::new())).await;
                    }
                    let _ = delete_file(&state.config.storage_dir, &raw_path).await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to generate unique short_id")),
                    );
                }
            }
        };

        // Is this the first thumbnail (and no primary exists)?
        let is_primary = audio_item.thumbnail_path.is_none() && uploaded_thumbnails.is_empty();
        let sort_order = (current_count + uploaded_thumbnails.len() as i64) as i32;

        // Insert with status='processing', all paths except raw_path are NULL initially
        let insert_result = sqlx::query(
            "INSERT INTO audio_thumbnails (audio_id, short_id, raw_path, thumbnail_path, preview_path, is_primary, sort_order, status) 
             VALUES (?, ?, ?, NULL, NULL, ?, ?, 'processing')"
        )
        .bind(id)
        .bind(&short_id)
        .bind(&raw_path)
        .bind(is_primary)
        .bind(sort_order)
        .execute(&state.db.pool)
        .await;

        match insert_result {
            Ok(res) => {
                let thumb_id = res.last_insert_id() as i32;
                uploaded_thumbnails.push(AudioThumbnail {
                    id: thumb_id,
                    audio_id: id,
                    short_id,
                    raw_path: Some(raw_path),
                    thumbnail_path: None,
                    preview_path: None,
                    is_primary,
                    sort_order,
                    status: "processing".to_string(),
                    created_at: DateTime::from(Utc::now()),
                });
            }
            Err(e) => {
                tracing::error!("Failed to insert thumbnail: {}", e);
                for item in &uploaded_thumbnails {
                    let _ = delete_file(&state.config.storage_dir, &item.raw_path.as_ref().unwrap_or(&String::new())).await;
                }
                let _ = delete_file(&state.config.storage_dir, &raw_path).await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to save thumbnail metadata")),
                );
            }
        }
    }

    tracing::info!(total_uploaded = uploaded_thumbnails.len(), "Thumbnails uploaded, spawning background processing");

    // PHASE 2: Spawn detached background task for thumbnail/preview generation
    let db_pool = state.db.pool.clone();
    let storage_dir = state.config.storage_dir.clone();
    let semaphore = state.image_semaphore.clone();
    let items_to_process = uploaded_thumbnails.clone();

    tokio::spawn(async move {
        for item in &items_to_process {
            let semaphore = semaphore.clone();
            let db_pool = db_pool.clone();
            let storage_dir = storage_dir.clone();
            let raw_path = item.raw_path.clone().unwrap_or_default();
            let item_id = item.id;
            let audio_id = item.audio_id;
            let is_primary = item.is_primary;

            let task = tokio::spawn(async move {
                // Read raw file
                let file_bytes = match read_file(&storage_dir, &raw_path).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::error!("Failed to read raw thumbnail: {}", e);
                        return;
                    }
                };

                // Acquire semaphore (memory ceiling)
                let _permit = match semaphore.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::error!("Semaphore closed");
                        return;
                    }
                };

                // Generate thumbnail + preview
                let file_bytes_clone = file_bytes.clone();
                let result = tokio::task::spawn_blocking(move || {
                    generate_thumbnail_and_preview(&file_bytes_clone)
                })
                .await;

                let (thumb_bytes, preview_bytes) = match result {
                    Ok(Ok((thumb, preview))) => (thumb, preview),
                    Ok(Err(e)) => {
                        tracing::error!("Failed to generate thumbnail/preview: {}", e);
                        let _ = sqlx::query("UPDATE audio_thumbnails SET status = 'failed_processing' WHERE id = ?")
                            .bind(item_id)
                            .execute(&db_pool)
                            .await;
                        return;
                    }
                    Err(e) => {
                        tracing::error!("Spawn blocking panicked: {}", e);
                        let _ = sqlx::query("UPDATE audio_thumbnails SET status = 'failed_processing' WHERE id = ?")
                            .bind(item_id)
                            .execute(&db_pool)
                            .await;
                        return;
                    }
                };

                // Generate storage paths for thumbnail and preview
                let thumbnail_path = generate_thumbnail_path(&raw_path);
                let preview_path = generate_preview_path(&raw_path);

                // Save thumbnail file
                let thumb_full_path = std::path::PathBuf::from(&storage_dir).join(&thumbnail_path);
                if let Err(e) = save_file(&thumb_full_path, &thumb_bytes).await {
                    tracing::error!("Failed to save thumbnail file: {}", e);
                    let _ = sqlx::query("UPDATE audio_thumbnails SET status = 'failed_processing' WHERE id = ?")
                        .bind(item_id)
                        .execute(&db_pool)
                        .await;
                    return;
                }

                // Save preview file
                let preview_full_path = std::path::PathBuf::from(&storage_dir).join(&preview_path);
                if let Err(e) = save_file(&preview_full_path, &preview_bytes).await {
                    tracing::error!("Failed to save preview file: {}", e);
                    let _ = delete_file(&storage_dir, &thumbnail_path).await;
                    let _ = sqlx::query("UPDATE audio_thumbnails SET status = 'failed_processing' WHERE id = ?")
                        .bind(item_id)
                        .execute(&db_pool)
                        .await;
                    return;
                }

                // Update database with paths and status='active'
                let update_result = sqlx::query(
                    "UPDATE audio_thumbnails SET thumbnail_path = ?, preview_path = ?, status = 'active' WHERE id = ?"
                )
                .bind(&thumbnail_path)
                .bind(&preview_path)
                .bind(item_id)
                .execute(&db_pool)
                .await;

                if let Err(e) = update_result {
                    tracing::error!("Failed to update thumbnail metadata: {}", e);
                    let _ = delete_file(&storage_dir, &thumbnail_path).await;
                    let _ = delete_file(&storage_dir, &preview_path).await;
                    return;
                }

                // If this is primary, mirror thumbnail_path to audio.thumbnail_path
                if is_primary {
                    let _ = sqlx::query("UPDATE audio SET thumbnail_path = ? WHERE id = ?")
                        .bind(&thumbnail_path)
                        .bind(audio_id)
                        .execute(&db_pool)
                        .await;
                }

                tracing::info!("Thumbnail processing completed for cover {}", item_id);
            });

            let _ = task.await;
        }
    });

    (StatusCode::ACCEPTED, Json(ApiResponse::success(uploaded_thumbnails)))
}

// GET /api/audio/:id/thumbnails - List all thumbnails for an audio item
async fn list_audio_thumbnails(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<Vec<AudioThumbnail>>>) {
    // Verify audio belongs to user
    let audio_item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let audio_item = match audio_item {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Audio not found")),
            );
        }
    };

    if audio_item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only view your own audio thumbnails")),
        );
    }

    let thumbnails: Result<Vec<AudioThumbnail>, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio_thumbnails WHERE audio_id = ? ORDER BY sort_order ASC, id ASC",
        AUDIO_THUMBNAIL_COLUMNS
    ))
    .bind(id)
    .fetch_all(&state.db.pool)
    .await;

    match thumbnails {
        Ok(thumbs) => (StatusCode::OK, Json(ApiResponse::success(thumbs))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch thumbnails")),
        ),
    }
}

// GET /api/audio/cover/{short_id_cover} - Serve raw cover image inline
async fn serve_audio_cover_raw(
    State(state): State<Arc<AppState>>,
    Path(short_id_cover): Path<String>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Join with audio table to get visibility and user_id
    let result: Result<(String, String, i32, String), _> = sqlx::query_as(
        "SELECT at.raw_path, a.visibility, a.user_id, at.status
         FROM audio_thumbnails at
         JOIN audio a ON a.id = at.audio_id
         WHERE at.short_id = ?"
    )
    .bind(&short_id_cover)
    .fetch_one(&state.db.pool)
    .await;

    let (raw_path, visibility, user_id, status) = match result {
        Ok(row) => row,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Cover image not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Check if still processing
    if status != "active" {
        return build_error_response(
            StatusCode::NOT_FOUND,
            "Cover image still processing",
            &headers,
            &state.config.frontend_url,
        );
    }

    // Access control based on visibility
    if visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private cover images",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This cover image is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    match read_file(&state.config.storage_dir, &raw_path).await {
        Ok(data) => {
            // Try to determine mime type from raw_path extension
            let mime_type = if let Some(ext) = raw_path.rsplit('.').next() {
                match ext.to_lowercase().as_str() {
                    "jpg" | "jpeg" => "image/jpeg",
                    "png" => "image/png",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    _ => "application/octet-stream",
                }
            } else {
                "application/octet-stream"
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .header(
                    header::CONTENT_DISPOSITION,
                    "inline",
                )
                .body(Body::from(data))
                .unwrap()
                .into_response()
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Cover file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/audio/cover/t/{short_id_cover} - Serve cover thumbnail (pre-generated WebP)
// GET /api/audio/cover/t/{short_id_audio}?primary=true - Serve primary cover thumbnail
async fn serve_audio_cover_thumbnail(
    State(state): State<Arc<AppState>>,
    Path(short_id_cover): Path<String>,
    Query(query): Query<CoverThumbnailQuery>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    // If primary=true, resolve short_id_audio to primary cover short_id
    let actual_short_id = if query.primary.unwrap_or(false) {
        // short_id_cover is actually short_id_audio in this case
        let result: Result<Option<String>, _> = sqlx::query_scalar(
            "SELECT at.short_id FROM audio_thumbnails at
             JOIN audio a ON a.id = at.audio_id
             WHERE a.short_id = ? AND at.is_primary = true"
        )
        .bind(&short_id_cover)
        .fetch_optional(&state.db.pool)
        .await;

        match result {
            Ok(Some(sid)) => sid,
            Ok(None) => {
                return build_error_response(
                    StatusCode::NOT_FOUND,
                    "No primary cover found for this audio item",
                    &headers,
                    &state.config.frontend_url,
                );
            }
            Err(_) => {
                return build_error_response(
                    StatusCode::NOT_FOUND,
                    "Audio item not found",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    } else {
        short_id_cover
    };

    // Join with audio table to get visibility and user_id
    let result: Result<(Option<String>, String, i32, String), _> = sqlx::query_as(
        "SELECT at.thumbnail_path, a.visibility, a.user_id, at.status
         FROM audio_thumbnails at
         JOIN audio a ON a.id = at.audio_id
         WHERE at.short_id = ?"
    )
    .bind(&actual_short_id)
    .fetch_one(&state.db.pool)
    .await;

    let (thumbnail_path, visibility, user_id, status) = match result {
        Ok(row) => row,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Cover image not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Check if still processing or no thumbnail yet
    if status != "active" || thumbnail_path.is_none() {
        return build_error_response(
            StatusCode::NOT_FOUND,
            "Cover thumbnail still processing",
            &headers,
            &state.config.frontend_url,
        );
    }

    // Access control based on visibility
    if visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private cover images",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This cover image is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    let thumb_path = thumbnail_path.unwrap_or_default();
    match read_file(&state.config.storage_dir, &thumb_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=31536000") // 1 year
            .body(Body::from(data))
            .unwrap()
            .into_response(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Cover thumbnail file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/audio/cover/p/{short_id_cover} - Serve cover preview (pre-generated WebP, larger than thumbnail)
async fn serve_audio_cover_preview(
    State(state): State<Arc<AppState>>,
    Path(short_id_cover): Path<String>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Join with audio table to get visibility and user_id
    let result: Result<(Option<String>, String, i32, String), _> = sqlx::query_as(
        "SELECT at.preview_path, a.visibility, a.user_id, at.status
         FROM audio_thumbnails at
         JOIN audio a ON a.id = at.audio_id
         WHERE at.short_id = ?"
    )
    .bind(&short_id_cover)
    .fetch_one(&state.db.pool)
    .await;

    let (preview_path, visibility, user_id, status) = match result {
        Ok(row) => row,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Cover image not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Check if still processing or no preview yet
    if status != "active" || preview_path.is_none() {
        return build_error_response(
            StatusCode::NOT_FOUND,
            "Cover preview still processing",
            &headers,
            &state.config.frontend_url,
        );
    }

    // Access control based on visibility
    if visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private cover images",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This cover image is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    let prev_path = preview_path.unwrap_or_default();
    match read_file(&state.config.storage_dir, &prev_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=3600") // 1 hour
            .body(Body::from(data))
            .unwrap()
            .into_response(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Cover preview file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/audio/{short_id_audio}/cover/{short_id_cover} - Serve scoped cover thumbnail (replaces old /audio/{id}/thumbnails/{thumbnail_id})
async fn get_audio_cover_scoped(
    State(state): State<Arc<AppState>>,
    Path((short_id_audio, short_id_cover)): Path<(String, String)>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Look up audio by short_id
    let audio_result: Result<(i32, String, i32), _> = sqlx::query_as(
        "SELECT id, visibility, user_id FROM audio WHERE short_id = ?"
    )
    .bind(&short_id_audio)
    .fetch_one(&state.db.pool)
    .await;

    let (audio_id, visibility, user_id) = match audio_result {
        Ok(row) => row,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Audio not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Look up cover by short_id AND audio_id (prevents cross-audio cover access)
    let cover_result: Result<(Option<String>, String), _> = sqlx::query_as(
        "SELECT thumbnail_path, status FROM audio_thumbnails WHERE short_id = ? AND audio_id = ?"
    )
    .bind(&short_id_cover)
    .bind(audio_id)
    .fetch_one(&state.db.pool)
    .await;

    let (thumbnail_path, status) = match cover_result {
        Ok(row) => row,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Cover image not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Check if still processing or no thumbnail yet
    if status != "active" || thumbnail_path.is_none() {
        return build_error_response(
            StatusCode::NOT_FOUND,
            "Cover image still processing",
            &headers,
            &state.config.frontend_url,
        );
    }

    // Access control based on visibility
    if visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private cover images",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This cover image is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    let thumb_path = thumbnail_path.unwrap_or_default();
    match read_file(&state.config.storage_dir, &thumb_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=31536000")
            .body(Body::from(data))
            .unwrap()
            .into_response(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Thumbnail file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// PATCH /api/audio/:id/thumbnails/:thumbnail_id - Set a thumbnail as primary
async fn set_primary_audio_thumbnail(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(req): Json<SetPrimaryCoverRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Verify audio belongs to user
    let audio_item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let audio_item = match audio_item {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Audio not found")),
            );
        }
    };

    if audio_item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only modify your own audio covers")),
        );
    }

    // Verify cover image belongs to this audio and get its numeric id, thumbnail_path and status
    let thumbnail: Result<(i32, Option<String>), _> = sqlx::query_as(
        "SELECT id, thumbnail_path FROM audio_thumbnails WHERE short_id = ? AND audio_id = ?"
    )
    .bind(&req.short_id)
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let (thumbnail_id, thumbnail_path) = match thumbnail {
        Ok(t) => t,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Cover image not found")),
            );
        }
    };

    // Start transaction
    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to update primary cover")),
            );
        }
    };

    // Unset all other covers as primary for this audio
    if let Err(e) = sqlx::query("UPDATE audio_thumbnails SET is_primary = FALSE WHERE audio_id = ? AND id != ?")
        .bind(id)
        .bind(thumbnail_id)
        .execute(&mut *tx)
        .await
    {
        let _ = tx.rollback().await;
        tracing::error!("Failed to unset other covers: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update primary cover")),
        );
    }

    // Set this cover as primary
    if let Err(e) = sqlx::query("UPDATE audio_thumbnails SET is_primary = TRUE WHERE id = ?")
        .bind(thumbnail_id)
        .execute(&mut *tx)
        .await
    {
        let _ = tx.rollback().await;
        tracing::error!("Failed to set primary cover: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update primary cover")),
        );
    }

    // Update audio.thumbnail_path to point to this cover ONLY if it's been processed (thumbnail_path is not NULL)
    // Skip mirroring if still processing (thumbnail_path = NULL) to avoid writing NULL over a working thumbnail
    if let Some(path) = thumbnail_path {
        if let Err(e) = sqlx::query("UPDATE audio SET thumbnail_path = ? WHERE id = ?")
            .bind(&path)
            .bind(id)
            .execute(&mut *tx)
            .await
        {
            let _ = tx.rollback().await;
            tracing::error!("Failed to update audio thumbnail_path: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to update primary cover")),
            );
        }
    } else {
        tracing::debug!("Skipping audio.thumbnail_path mirror: cover is still processing (thumbnail_path = NULL)");
    }

    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update primary cover")),
        );
    }

    (
        StatusCode::OK,
        Json(ApiResponse::success("Primary cover updated".to_string())),
    )
}

// DELETE /api/audio/:id/thumbnails/:thumbnail_id - Delete a specific thumbnail
async fn delete_audio_thumbnail(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Get short_id from query parameter
    let short_id_cover = match params.get("short_id") {
        Some(sid) => sid.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Missing query parameter: short_id")),
            );
        }
    };

    // Verify audio belongs to user
    let audio_item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE id = ?",
        AUDIO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let audio_item = match audio_item {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Audio not found")),
            );
        }
    };

    if audio_item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only delete your own audio covers")),
        );
    }

    // Fetch cover image to get its numeric id, paths and is_primary status
    let thumbnail: Result<(i32, Option<String>, Option<String>, Option<String>, bool), _> = sqlx::query_as(
        "SELECT id, raw_path, thumbnail_path, preview_path, is_primary FROM audio_thumbnails WHERE short_id = ? AND audio_id = ?"
    )
    .bind(&short_id_cover)
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let (thumbnail_id, raw_path, thumbnail_path, preview_path, is_primary) = match thumbnail {
        Ok(t) => t,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Cover image not found")),
            );
        }
    };

    // Delete from database
    let result = sqlx::query("DELETE FROM audio_thumbnails WHERE id = ?")
        .bind(thumbnail_id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(_) => {
            // Delete files from disk (raw, thumbnail, preview)
            if let Some(path) = raw_path {
                if let Err(e) = delete_file(&state.config.storage_dir, &path).await {
                    tracing::warn!("Failed to delete raw cover file from disk: {}", e);
                }
            }
            if let Some(path) = &thumbnail_path {
                if let Err(e) = delete_file(&state.config.storage_dir, path).await {
                    tracing::warn!("Failed to delete cover thumbnail file from disk: {}", e);
                }
            }
            if let Some(path) = preview_path {
                if let Err(e) = delete_file(&state.config.storage_dir, &path).await {
                    tracing::warn!("Failed to delete cover preview file from disk: {}", e);
                }
            }

            // If this was the primary cover, find the next one and set it as primary
            if is_primary {
                let next_thumbnail: Result<Option<(i32, Option<String>)>, _> = sqlx::query_as(
                    "SELECT id, thumbnail_path FROM audio_thumbnails WHERE audio_id = ? ORDER BY sort_order ASC LIMIT 1"
                )
                .bind(id)
                .fetch_optional(&state.db.pool)
                .await;

                if let Ok(Some((next_id, next_path))) = next_thumbnail {
                    let _ = sqlx::query("UPDATE audio_thumbnails SET is_primary = TRUE WHERE id = ?")
                        .bind(next_id)
                        .execute(&state.db.pool)
                        .await;
                    if let Some(path) = next_path {
                        let _ = sqlx::query("UPDATE audio SET thumbnail_path = ? WHERE id = ?")
                            .bind(&path)
                            .bind(id)
                            .execute(&state.db.pool)
                            .await;
                    }
                } else {
                    // No more covers, clear audio.thumbnail_path
                    let _ = sqlx::query("UPDATE audio SET thumbnail_path = NULL WHERE id = ?")
                        .bind(id)
                        .execute(&state.db.pool)
                        .await;
                }
            }

            (
                StatusCode::OK,
                Json(ApiResponse::success("Cover image deleted".to_string())),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to delete cover image")),
        ),
    }
}
// GET /api/audio/r/:short_id - Stream audio file with HTTP 206 Range support
async fn serve_audio_stream(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    use axum::http::header;
    use axum::response::{Response, IntoResponse};
    use axum::body::Body;

    let item: Result<AudioItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM audio WHERE short_id = ?",
        AUDIO_COLUMNS
    ))
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    let item = match item {
        Ok(item) => item,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Audio not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Access control for private audio
    if item.visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if item.user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private audio",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This audio is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    let full_path = std::path::PathBuf::from(&state.config.storage_dir).join(&item.stored_path);

    let file_metadata = match tokio::fs::metadata(&full_path).await {
        Ok(m) => m,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Audio file not found on disk",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    let file_size = file_metadata.len();
    let content_type = &item.mime_type;

    // Parse Range header for HTTP 206 Partial Content
    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().unwrap_or("");
        if let Some(range) = crate::media::parse_range_header(range_str, file_size) {
            let (start, end) = range;
            let chunk_size = end - start + 1;

            // Read the requested range from file
            let mut file = match tokio::fs::File::open(&full_path).await {
                Ok(f) => f,
                Err(_) => {
                    return build_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to open audio file",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            };

            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            if let Err(_) = file.seek(std::io::SeekFrom::Start(start)).await {
                return build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to seek in audio file",
                    &headers,
                    &state.config.frontend_url,
                );
            }

            let mut buffer = vec![0u8; chunk_size as usize];
            if let Err(_) = file.read_exact(&mut buffer).await {
                return build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to read audio file range",
                    &headers,
                    &state.config.frontend_url,
                );
            }

            return Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, chunk_size.to_string())
                .header(header::ACCEPT_RANGES, "bytes")
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, file_size),
                )
                .body(Body::from(buffer))
                .unwrap()
                .into_response();
        }
    }

    // No Range header — serve full file
    match tokio::fs::read(&full_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, file_size.to_string())
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::from(data))
            .unwrap()
            .into_response(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Audio file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}
