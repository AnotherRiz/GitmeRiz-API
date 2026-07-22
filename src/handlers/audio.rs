use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, delete},
    Extension, Json, Router,
};
use serde::Serialize;
use sqlx::FromRow;
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::auth::validate_token;
use crate::error_page::build_error_response;
use crate::media::{
    delete_file, generate_storage_path, generate_thumbnail_only, generate_thumbnail_path,
    read_file, save_file, validate_extension, MediaType,
};
use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

// ─── Constants ─────────────────────────────────────────────────────────────────

/// Column list used in all SELECT queries (keep in sync with AudioItem struct)
const AUDIO_COLUMNS: &str = "id, user_id, title, description, original_filename, stored_path, size_bytes, mime_type, visibility, thumbnail_path";

/// Allowed extensions for the optional audio cover art thumbnail
const ALLOWED_THUMBNAIL_EXTENSIONS: &[&str] = &[".jpg", ".jpeg", ".png", ".webp", ".gif"];

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
}

// ─── Routes ────────────────────────────────────────────────────────────────────

/// Public routes (no auth required, but private items check cookie/header)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audio/public", get(list_public_audio))
        .route("/audio/{id}", get(get_audio))
        .route("/audio/{id}/download", get(download_audio))
        .route("/audio/{id}/thumbnail", get(serve_audio_thumbnail))
}

/// Protected routes (require auth middleware)
pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audio", get(list_audio).post(upload_audio))
        .route("/audio/{id}", delete(delete_audio))
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

// GET /api/audio - List audio (superuser sees all, others see only their own)
async fn list_audio(
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

    // Process optional thumbnail (cover art). Failure here is non-fatal — the audio
    // upload still succeeds, just without a thumbnail.
    let mut thumbnail_path: Option<String> = None;
    if let (Some(thumb_bytes), Some(thumb_filename)) = (thumbnail_data, thumbnail_filename) {
        let thumb_ext = thumb_filename
            .rsplit('.')
            .next()
            .map(|e| format!(".{}", e.to_lowercase()));

        let is_allowed_ext = thumb_ext
            .as_deref()
            .map(|e| ALLOWED_THUMBNAIL_EXTENSIONS.contains(&e))
            .unwrap_or(false);

        if is_allowed_ext {
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
        } else {
            tracing::warn!(
                "Skipping audio thumbnail: unsupported extension '{:?}'",
                thumb_ext
            );
        }
    }

    // Insert into database
    let result = sqlx::query(
        "INSERT INTO audio (user_id, title, description, original_filename, stored_path, size_bytes, mime_type, visibility, thumbnail_path) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
async fn serve_audio_thumbnail(
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

    let thumb_path = match &item.thumbnail_path {
        Some(p) => p,
        None => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "This audio has no thumbnail",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    match read_file(&state.config.storage_dir, thumb_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "public, max-age=31536000") // 1 year
            .body(Body::from(data))
            .unwrap()
            .into_response(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Thumbnail not found on disk",
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

            let result = sqlx::query("DELETE FROM audio WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    if let Err(e) = delete_file(&state.config.storage_dir, &item.stored_path).await {
                        tracing::warn!("Failed to delete file from disk: {}", e);
                    }
                    if let Some(thumb) = &item.thumbnail_path {
                        if let Err(e) = delete_file(&state.config.storage_dir, thumb).await {
                            tracing::warn!("Failed to delete thumbnail from disk: {}", e);
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
