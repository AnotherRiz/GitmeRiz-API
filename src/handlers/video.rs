use std::sync::Arc;
use std::collections::HashMap;
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_cookies::Cookies;
use tracing::Instrument;

use crate::auth::validate_token;
use crate::error_page::build_error_response;
use crate::media::{
    delete_file, generate_short_id, generate_storage_path, generate_thumbnail_path,
    generate_transcoded_path, get_extension, is_web_safe_video, save_file_streaming,
    validate_extension, MediaType,
};
use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

// ─── Data Structures ───────────────────────────────────────────────────────────

/// Column list used in all SELECT queries (keep in sync with VideoItem struct)
const VIDEO_COLUMNS: &str = "id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, transcoded_path, pinned, status, pin_order";

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct VideoItem {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    pub original_filename: String,
    pub stored_path: String,
    pub size_bytes: i64,
    pub mime_type: String,
    pub visibility: String,
    pub short_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoded_path: Option<String>,
    pub pinned: bool,
    pub status: String,
    pub pin_order: i32,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UploadResponse {
    Single(VideoItem),
    Bulk(Vec<VideoItem>),
}

#[derive(Debug, Deserialize)]
struct VideoPageQuery {
    cursor: Option<i32>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct VideoPageResponse {
    items: Vec<VideoItem>,
    next_cursor: Option<i32>,
    limit: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateVideoRequest {
    title: Option<String>,
    visibility: Option<String>,
    pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StatusCheckRequest {
    ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
struct ReorderPinsRequest {
    ordered_ids: Vec<i32>,
}



// ─── Routes ────────────────────────────────────────────────────────────────────

/// Public routes (no auth required, but private items check cookie/header/signed URL)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/video/public", get(list_public_videos))
        .route("/video/{id}", get(get_video))
        .route("/video/d/{id}", get(download_video))
        .route("/video/r/{short_id}", get(serve_video_stream))
        .route("/video/t/{short_id}", get(serve_video_thumbnail))
}

/// Protected routes (require auth middleware)
pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/video", post(upload_video))
        .route("/video/me", get(list_my_videos))
        .route("/video/me/pinned", get(list_pinned_videos))
        .route("/video/status", post(check_status))
        .route("/video/reorder-pins", patch(reorder_pins))
        .route("/video/{id}", patch(update_video).delete(delete_video))
        .route("/video/{id}/reprocess", post(reprocess_video))
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


/// Determine the servable file path for a video (transcoded if available, otherwise original)
fn get_servable_path(item: &VideoItem) -> &str {
    item.transcoded_path.as_deref().unwrap_or(&item.stored_path)
}

/// Determine the servable mime type (transcoded = mp4, otherwise original)
fn get_servable_mime(item: &VideoItem) -> &str {
    if item.transcoded_path.is_some() {
        "video/mp4"
    } else {
        &item.mime_type
    }
}

/// Run FFmpeg to extract a video thumbnail (WebP, max 500px width).
/// Tries -ss 00:00:01 first, then falls back to 00:00:00 for very short videos.
async fn ffmpeg_extract_thumbnail(input_path: &str, output_path: &str) -> Result<(), String> {
    // First attempt: extract at 1 second
    let result = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-ss", "00:00:01",
            "-i", input_path,
            "-vframes", "1",
            "-vf", "scale=1280:-1",
            "-c:v", "libwebp",
            "-quality", "90",
            output_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn ffmpeg for thumbnail: {}", e))?;

    if result.success() {
        // Verify the output file was actually created (ffmpeg sometimes exits 0 but produces nothing)
        if tokio::fs::metadata(output_path).await.is_ok() {
            return Ok(());
        }
    }

    // Fallback: extract at 0 seconds (for very short videos)
    tracing::warn!("Thumbnail extraction at 00:00:01 failed, retrying at 00:00:00");
    let fallback = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-ss", "00:00:00",
            "-i", input_path,
            "-vframes", "1",
            "-vf", "scale=1280:-1",
            "-c:v", "libwebp",
            "-quality", "90",
            output_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn ffmpeg for thumbnail (fallback): {}", e))?;

    if fallback.success() && tokio::fs::metadata(output_path).await.is_ok() {
        Ok(())
    } else {
        Err("FFmpeg thumbnail extraction failed at both 00:00:01 and 00:00:00".to_string())
    }
}

/// Run FFmpeg to transcode a video to web-safe MP4 (H.264 + AAC + faststart).
async fn ffmpeg_transcode_to_mp4(input_path: &str, output_path: &str) -> Result<(), String> {
    let result = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input_path,
            "-c:v", "libx264",
            "-preset", "medium",
            "-crf", "23",
            "-c:a", "aac",
            "-b:a", "128k",
            "-movflags", "+faststart",
            output_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn ffmpeg for transcoding: {}", e))?;

    if result.success() {
        Ok(())
    } else {
        Err("FFmpeg transcoding to MP4 failed".to_string())
    }
}

// ─── Handlers ──────────────────────────────────────────────────────────────────

// GET /video/public — List all public videos with cursor-based pagination
async fn list_public_videos(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VideoPageQuery>,
) -> (StatusCode, Json<ApiResponse<VideoPageResponse>>) {
    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let fetch_limit = limit + 1;

    let items: Result<Vec<VideoItem>, _> = if let Some(cursor) = query.cursor {
        sqlx::query_as(&format!(
            "SELECT {} FROM videos WHERE visibility = 'public' AND id < ? ORDER BY id DESC LIMIT ?",
            VIDEO_COLUMNS
        ))
        .bind(cursor)
        .bind(fetch_limit)
        .fetch_all(&state.db.pool)
        .await
    } else {
        sqlx::query_as(&format!(
            "SELECT {} FROM videos WHERE visibility = 'public' ORDER BY id DESC LIMIT ?",
            VIDEO_COLUMNS
        ))
        .bind(fetch_limit)
        .fetch_all(&state.db.pool)
        .await
    };

    match items {
        Ok(mut items) => {
            let has_more = items.len() as i64 > limit;
            let next_cursor = if has_more {
                items.pop();
                items.last().map(|item| item.id)
            } else {
                None
            };

            (StatusCode::OK, Json(ApiResponse::success(VideoPageResponse {
                items,
                next_cursor,
                limit,
            })))
        }
        Err(e) => {
            tracing::error!("Failed to fetch public videos: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error("Failed to fetch videos")))
        }
    }
}

// GET /video/me — List current user's videos with cursor-based pagination
async fn list_my_videos(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Query(query): Query<VideoPageQuery>,
) -> (StatusCode, Json<ApiResponse<VideoPageResponse>>) {
    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let fetch_limit = limit + 1;

    let items: Result<Vec<VideoItem>, _> = if let Some(cursor) = query.cursor {
        sqlx::query_as(&format!(
            "SELECT {} FROM videos WHERE user_id = ? AND id < ? ORDER BY id DESC LIMIT ?",
            VIDEO_COLUMNS
        ))
        .bind(auth_user.id)
        .bind(cursor)
        .bind(fetch_limit)
        .fetch_all(&state.db.pool)
        .await
    } else {
        sqlx::query_as(&format!(
            "SELECT {} FROM videos WHERE user_id = ? ORDER BY id DESC LIMIT ?",
            VIDEO_COLUMNS
        ))
        .bind(auth_user.id)
        .bind(fetch_limit)
        .fetch_all(&state.db.pool)
        .await
    };

    match items {
        Ok(mut items) => {
            let has_more = items.len() as i64 > limit;
            let next_cursor = if has_more {
                items.pop();
                items.last().map(|item| item.id)
            } else {
                None
            };

            (StatusCode::OK, Json(ApiResponse::success(VideoPageResponse {
                items,
                next_cursor,
                limit,
            })))
        }
        Err(e) => {
            tracing::error!("Failed to fetch user videos: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error("Failed to fetch videos")))
        }
    }
}

// GET /video/me/pinned — List pinned videos for authenticated user
async fn list_pinned_videos(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<VideoItem>>>) {
    let items: Result<Vec<VideoItem>, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE user_id = ? AND pinned = TRUE ORDER BY pin_order ASC, updated_at DESC",
        VIDEO_COLUMNS
    ))
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(e) => {
            tracing::error!("Failed to fetch pinned videos: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error("Failed to fetch pinned videos")))
        }
    }
}

// POST /video — Upload video file(s) with streaming to disk
async fn upload_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse<UploadResponse>>) {
    use uuid::Uuid;

    let batch_id = Uuid::new_v4();

    let mut title: Option<String> = None;
    let mut visibility = "private".to_string();

    // Struct to hold saved file info before DB insert
    struct SavedFile {
        original_filename: String,
        stored_path: String,
        full_path: std::path::PathBuf,
        size_bytes: u64,
        extension: String,
        mime_type: &'static str,
    }

    let mut saved_files: Vec<SavedFile> = Vec::new();

    // Parse multipart fields — stream file chunks directly to disk
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "title" => {
                if let Ok(text) = field.text().await {
                    title = Some(text);
                }
            }
            "visibility" => {
                if let Ok(text) = field.text().await {
                    let val = text.trim().to_lowercase();
                    if val == "public" || val == "private" {
                        visibility = val;
                    }
                }
            }
            "file" => {
                let orig_filename = match field.file_name().map(|s| s.to_string()) {
                    Some(name) if !name.is_empty() => name,
                    _ => continue,
                };

                // Validate extension
                let extension = match validate_extension(MediaType::Video, &orig_filename) {
                    Ok(ext) => ext,
                    Err(msg) => {
                        // Cleanup already-saved files on validation error
                        for sf in &saved_files {
                            let _ = tokio::fs::remove_file(&sf.full_path).await;
                        }
                        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(msg)));
                    }
                };

                // Enforce bulk upload limit (max 5 files)
                if saved_files.len() >= 5 {
                    for sf in &saved_files {
                        let _ = tokio::fs::remove_file(&sf.full_path).await;
                    }
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error("Too many files. Limit is 5 video files per upload.")),
                    );
                }

                // Generate storage path and stream to disk
                let (stored_path, full_path) =
                    generate_storage_path(&state.config.storage_dir, MediaType::Video, &extension);

                let size_bytes = match save_file_streaming(&full_path, &mut field).await {
                    Ok(size) => size,
                    Err(e) => {
                        tracing::error!("Failed to stream file to disk: {}", e);
                        for sf in &saved_files {
                            let _ = tokio::fs::remove_file(&sf.full_path).await;
                        }
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to save file to disk")),
                        );
                    }
                };

                if size_bytes == 0 {
                    let _ = tokio::fs::remove_file(&full_path).await;
                    continue;
                }

                let mime_type = MediaType::Video.mime_type_for_extension(&extension);

                saved_files.push(SavedFile {
                    original_filename: orig_filename,
                    stored_path,
                    full_path,
                    size_bytes,
                    extension,
                    mime_type,
                });
            }
            _ => {}
        }
    }

    if saved_files.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error("No file provided")));
    }

    let num_files = saved_files.len();
    tracing::info!(%batch_id, file_count = num_files, "Starting video upload with background processing");

    // ── PHASE 1: Save metadata to DB (fast) ──────────────────────────────────
    let mut uploaded_items: Vec<VideoItem> = Vec::new();
    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {:?}", e);
            for sf in &saved_files {
                let _ = tokio::fs::remove_file(&sf.full_path).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to start database transaction")),
            );
        }
    };

    for sf in &saved_files {
        let item_title = if num_files == 1 && title.is_some() {
            title.clone().unwrap()
        } else {
            sf.original_filename.clone()
        };

        // Generate unique short_id
        let short_id = loop {
            let candidate = generate_short_id();
            let exists: Result<Option<(i32,)>, _> =
                sqlx::query_as("SELECT id FROM videos WHERE short_id = ?")
                    .bind(&candidate)
                    .fetch_optional(&mut *tx)
                    .await;

            match exists {
                Ok(None) => break candidate,
                Ok(Some(_)) => continue,
                Err(e) => {
                    tracing::error!("Failed to check short_id uniqueness: {}", e);
                    for sf2 in &saved_files {
                        let _ = tokio::fs::remove_file(&sf2.full_path).await;
                    }
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to generate unique short_id")),
                    );
                }
            }
        };

        let result = sqlx::query(
            "INSERT INTO videos (user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'processing')",
        )
        .bind(auth_user.id)
        .bind(&item_title)
        .bind(&sf.original_filename)
        .bind(&sf.stored_path)
        .bind(sf.size_bytes as i64)
        .bind(sf.mime_type)
        .bind(&visibility)
        .bind(&short_id)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(res) => {
                uploaded_items.push(VideoItem {
                    id: res.last_insert_id() as i32,
                    user_id: auth_user.id,
                    title: item_title,
                    original_filename: sf.original_filename.clone(),
                    stored_path: sf.stored_path.clone(),
                    size_bytes: sf.size_bytes as i64,
                    mime_type: sf.mime_type.to_string(),
                    visibility: visibility.clone(),
                    short_id,
                    thumbnail_path: None,
                    transcoded_path: None,
                    pinned: false,
                    status: "processing".to_string(),
                    pin_order: 0,
                });
            }
            Err(e) => {
                tracing::error!("Failed to insert video item: {}", e);
                for sf2 in &saved_files {
                    let _ = tokio::fs::remove_file(&sf2.full_path).await;
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to save video metadata")),
                );
            }
        }
    }

    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {:?}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit database transaction")),
        );
    }

    tracing::info!(%batch_id, total_uploaded = uploaded_items.len(), "Raw video files saved, spawning FFmpeg background processing");

    // ── PHASE 2: Spawn background FFmpeg processing ──────────────────────────
    let db_pool = state.db.pool.clone();
    let storage_dir = state.config.storage_dir.clone();
    let semaphore = state.video_semaphore.clone();

    // Collect processing info from saved_files + uploaded_items
    let process_items: Vec<(i32, String, String, String)> = uploaded_items
        .iter()
        .zip(saved_files.iter())
        .map(|(item, sf)| (item.id, sf.stored_path.clone(), sf.extension.clone(), sf.original_filename.clone()))
        .collect();

    tokio::spawn(async move {
        tracing::info!(%batch_id, "Video background processing started");

        for (item_id, stored_path, extension, filename) in &process_items {
            let _permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    tracing::error!(%batch_id, item_id, "Failed to acquire video processing semaphore");
                    let _ = sqlx::query("UPDATE videos SET status = 'failed_processing' WHERE id = ?")
                        .bind(item_id)
                        .execute(&db_pool)
                        .await;
                    continue;
                }
            };

            let input_full_path = std::path::PathBuf::from(&storage_dir).join(stored_path);
            let input_path_str = input_full_path.to_string_lossy().to_string();

            // Step 1: Thumbnail extraction
            let thumb_relative = generate_thumbnail_path(stored_path);
            let thumb_full_path = std::path::PathBuf::from(&storage_dir).join(&thumb_relative);

            // Ensure parent directory exists
            if let Some(parent) = thumb_full_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let thumb_result = ffmpeg_extract_thumbnail(
                &input_path_str,
                &thumb_full_path.to_string_lossy(),
            )
            .await;

            let final_thumb = if thumb_result.is_ok() {
                tracing::info!(%batch_id, item_id, %filename, "Thumbnail extracted successfully");
                Some(thumb_relative)
            } else {
                tracing::warn!(%batch_id, item_id, %filename, "Thumbnail extraction failed: {:?}", thumb_result.err());
                None
            };

            // Step 2: Transcoding (if not web-safe)
            let final_transcoded = if !is_web_safe_video(extension) {
                let transcoded_relative = generate_transcoded_path(stored_path);
                let transcoded_full_path = std::path::PathBuf::from(&storage_dir).join(&transcoded_relative);

                if let Some(parent) = transcoded_full_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }

                let transcode_result = ffmpeg_transcode_to_mp4(
                    &input_path_str,
                    &transcoded_full_path.to_string_lossy(),
                )
                .await;

                if transcode_result.is_ok() {
                    tracing::info!(%batch_id, item_id, %filename, "Transcoding to MP4 successful");
                    Some(transcoded_relative)
                } else {
                    tracing::error!(%batch_id, item_id, %filename, "Transcoding failed: {:?}", transcode_result.err());
                    None
                }
            } else {
                tracing::info!(%batch_id, item_id, %filename, "Video is web-safe, skipping transcoding");
                None
            };

            // Step 3: Update database
            // Consider it successful if at least the thumbnail was generated
            // (transcoding failure for non-web-safe is also tolerable — original can still be served)
            if final_thumb.is_some() || is_web_safe_video(extension) {
                let _ = sqlx::query(
                    "UPDATE videos SET status = 'active', thumbnail_path = ?, transcoded_path = ? WHERE id = ?"
                )
                .bind(&final_thumb)
                .bind(&final_transcoded)
                .bind(item_id)
                .execute(&db_pool)
                .await;

                tracing::info!(%batch_id, item_id, "Video activated");
            } else {
                let _ = sqlx::query("UPDATE videos SET status = 'failed_processing' WHERE id = ?")
                    .bind(item_id)
                    .execute(&db_pool)
                    .await;

                tracing::error!(%batch_id, item_id, "Video processing failed");
            }
        }

        tracing::info!(%batch_id, "Video background processing fully completed");
    }.instrument(tracing::info_span!("video_bg_process", %batch_id)));

    // ── PHASE 3: Return 202 Accepted immediately ─────────────────────────────
    if num_files == 1 {
        let single_item = uploaded_items.into_iter().next().unwrap();
        (StatusCode::ACCEPTED, Json(ApiResponse::success(UploadResponse::Single(single_item))))
    } else {
        (StatusCode::ACCEPTED, Json(ApiResponse::success(UploadResponse::Bulk(uploaded_items))))
    }
}

// POST /video/status — Check processing status of multiple videos
async fn check_status(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StatusCheckRequest>,
) -> impl IntoResponse {
    if payload.ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<HashMap<i32, String>>::error("No IDs provided")),
        );
    }

    if payload.ids.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<HashMap<i32, String>>::error("Too many IDs (max 100)")),
        );
    }

    let placeholders = payload.ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query = format!(
        "SELECT id, status FROM videos WHERE id IN ({}) AND user_id = ?",
        placeholders
    );

    let mut query_builder = sqlx::query_as::<_, (i32, String)>(&query);
    for id in &payload.ids {
        query_builder = query_builder.bind(id);
    }
    query_builder = query_builder.bind(auth_user.id);

    match query_builder.fetch_all(&state.db.pool).await {
        Ok(rows) => {
            let status_map: HashMap<i32, String> = rows.into_iter().collect();
            (StatusCode::OK, Json(ApiResponse::success(status_map)))
        }
        Err(e) => {
            tracing::error!("Failed to check video status: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<HashMap<i32, String>>::error("Failed to check status")),
            )
        }
    }
}

// GET /video/{id} — Get video metadata (public endpoint)
async fn get_video(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<VideoItem>>) {
    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE id = ?",
        VIDEO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::success(item))),
        Err(_) => (StatusCode::NOT_FOUND, Json(ApiResponse::error("Video not found"))),
    }
}

// GET /video/d/{id} — Download video file (attachment header)
async fn download_video(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE id = ?",
        VIDEO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            let serve_path = get_servable_path(&item);
            match crate::media::read_file(&state.config.storage_dir, serve_path).await {
                Ok(data) => {
                    let body = Body::from(data);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, get_servable_mime(&item))
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", item.original_filename),
                        )
                        .body(body)
                        .unwrap()
                }
                Err(_) => (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::<()>::error("File not found on disk")),
                )
                    .into_response(),
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error("Video not found")),
        )
            .into_response(),
    }
}

// GET /video/r/{short_id} — Serve video inline with HTTP Range support for streaming
async fn serve_video_stream(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE short_id = ?",
        VIDEO_COLUMNS
    ))
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    let item = match item {
        Ok(item) => item,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Video not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Access control for private videos
    if item.visibility == "private" {
        // Fall back to cookie/header auth
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if item.user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private videos",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This video is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    // Resolve the file to serve (transcoded if available)
    let serve_path = get_servable_path(&item);
    let full_path = std::path::PathBuf::from(&state.config.storage_dir).join(serve_path);

    let file_metadata = match tokio::fs::metadata(&full_path).await {
        Ok(m) => m,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Video file not found on disk",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    let file_size = file_metadata.len();
    let content_type = get_servable_mime(&item);

    // Parse Range header for HTTP 206 Partial Content
    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().unwrap_or("");
        if let Some(range) = parse_range_header(range_str, file_size) {
            let (start, end) = range;
            let chunk_size = end - start + 1;

            // Read the requested range from file
            let mut file = match tokio::fs::File::open(&full_path).await {
                Ok(f) => f,
                Err(_) => {
                    return build_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to open video file",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            };

            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            if let Err(_) = file.seek(std::io::SeekFrom::Start(start)).await {
                return build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to seek in video file",
                    &headers,
                    &state.config.frontend_url,
                );
            }

            let mut buffer = vec![0u8; chunk_size as usize];
            if let Err(_) = file.read_exact(&mut buffer).await {
                return build_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to read video file range",
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
                .unwrap();
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
            .unwrap(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Video file not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

/// Parse a Range header like "bytes=0-1023" into (start, end) tuple
fn parse_range_header(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    let range_str = range_str.strip_prefix("bytes=")?;
    let mut parts = range_str.splitn(2, '-');
    let start_str = parts.next()?;
    let end_str = parts.next()?;

    let start: u64 = if start_str.is_empty() {
        // Suffix range: e.g. bytes=-500 (last 500 bytes)
        let suffix: u64 = end_str.parse().ok()?;
        file_size.saturating_sub(suffix)
    } else {
        start_str.parse().ok()?
    };

    let end: u64 = if end_str.is_empty() {
        file_size - 1
    } else {
        end_str.parse().ok()?
    };

    // Clamp end to file boundary
    let end = end.min(file_size - 1);

    if start <= end && start < file_size {
        Some((start, end))
    } else {
        None
    }
}

// GET /video/t/{short_id} — Serve video thumbnail (WebP)
async fn serve_video_thumbnail(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE short_id = ?",
        VIDEO_COLUMNS
    ))
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    let item = match item {
        Ok(item) => item,
        Err(_) => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Video not found",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    // Access control (same rules as /video/r/)
    if item.visibility == "private" {
        let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
        match auth_user {
            Some(user) => {
                if item.user_id != user.id && !user.is_superuser() {
                    return build_error_response(
                        StatusCode::FORBIDDEN,
                        "You can only access your own private videos",
                        &headers,
                        &state.config.frontend_url,
                    );
                }
            }
            None => {
                return build_error_response(
                    StatusCode::UNAUTHORIZED,
                    "This video is private. Authentication required.",
                    &headers,
                    &state.config.frontend_url,
                );
            }
        }
    }

    // Serve thumbnail (or return 404 if not yet generated)
    let thumb_path = match &item.thumbnail_path {
        Some(p) => p,
        None => {
            return build_error_response(
                StatusCode::NOT_FOUND,
                "Thumbnail not yet generated (video may still be processing)",
                &headers,
                &state.config.frontend_url,
            );
        }
    };

    match crate::media::read_file(&state.config.storage_dir, thumb_path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"thumb_{}\"", item.original_filename),
            )
            .header(header::CACHE_CONTROL, "public, max-age=31536000") // 1 year
            .body(Body::from(data))
            .unwrap(),
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Thumbnail not found on disk",
            &headers,
            &state.config.frontend_url,
        ),
    }
}



// PATCH /video/{id} — Unified partial update (title, visibility, pinned)
async fn update_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateVideoRequest>,
) -> (StatusCode, Json<ApiResponse<VideoItem>>) {
    const MAX_PINNED_VIDEOS: i64 = 4;

    if payload.title.is_none() && payload.visibility.is_none() && payload.pinned.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No fields to update")),
        );
    }

    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE id = ?",
        VIDEO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let mut item = match item {
        Ok(item) => item,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(ApiResponse::error("Video not found")));
        }
    };

    // Check ownership
    if item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only edit your own videos")),
        );
    }

    // Validate and apply title
    let new_title = if let Some(ref title) = payload.title {
        if title.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Title cannot be empty")),
            );
        }
        title.clone()
    } else {
        item.title.clone()
    };

    // Validate and apply visibility
    let new_visibility = if let Some(ref visibility) = payload.visibility {
        let val = visibility.trim().to_lowercase();
        if val != "public" && val != "private" {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Visibility must be 'public' or 'private'")),
            );
        }
        val
    } else {
        item.visibility.clone()
    };

    // Handle pinned logic
    let (new_pinned, new_pin_order) = if let Some(pinned_value) = payload.pinned {
        if pinned_value && !item.pinned {
            // Pinning: check limit
            let count_result: Result<(i64,), _> = sqlx::query_as(
                "SELECT COUNT(*) FROM videos WHERE user_id = ? AND pinned = TRUE",
            )
            .bind(auth_user.id)
            .fetch_one(&state.db.pool)
            .await;

            let count = match count_result {
                Ok((count,)) => count,
                Err(e) => {
                    tracing::error!("Failed to count pinned videos: {:?}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to check pinned count")),
                    );
                }
            };

            if count >= MAX_PINNED_VIDEOS {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::error(&format!(
                        "You can only pin up to {} videos. Please unpin another video first.",
                        MAX_PINNED_VIDEOS
                    ))),
                );
            }

            // Get max pin_order
            let max_order_result: Result<Option<(Option<i32>,)>, _> = sqlx::query_as(
                "SELECT MAX(pin_order) FROM videos WHERE user_id = ? AND pinned = TRUE",
            )
            .bind(auth_user.id)
            .fetch_optional(&state.db.pool)
            .await;

            let new_pin_order = match max_order_result {
                Ok(Some((Some(max_order),))) => max_order + 1,
                Ok(Some((None,))) | Ok(None) => 1,
                Err(e) => {
                    tracing::error!("Failed to get max pin_order: {:?}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to assign pin order")),
                    );
                }
            };

            (true, new_pin_order)
        } else if !pinned_value && item.pinned {
            (false, 0)
        } else {
            (item.pinned, item.pin_order)
        }
    } else {
        (item.pinned, item.pin_order)
    };

    // Update database
    let result = sqlx::query(
        "UPDATE videos SET title = ?, visibility = ?, pinned = ?, pin_order = ? WHERE id = ?",
    )
    .bind(&new_title)
    .bind(&new_visibility)
    .bind(new_pinned)
    .bind(new_pin_order)
    .bind(id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(_) => {
            item.title = new_title;
            item.visibility = new_visibility;
            item.pinned = new_pinned;
            item.pin_order = new_pin_order;
            (StatusCode::OK, Json(ApiResponse::success(item)))
        }
        Err(e) => {
            tracing::error!("Failed to update video: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to update video")),
            )
        }
    }
}

// PATCH /video/reorder-pins — Persist drag-and-drop order for pinned videos
async fn reorder_pins(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReorderPinsRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    if payload.ordered_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No video IDs provided")),
        );
    }

    if payload.ordered_ids.len() > 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Cannot reorder more than 4 pinned videos")),
        );
    }

    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to start database transaction")),
            );
        }
    };

    // Verify all videos exist, belong to user, and are pinned
    for id in &payload.ordered_ids {
        let check_result: Result<Option<(i32, bool)>, _> =
            sqlx::query_as("SELECT user_id, pinned FROM videos WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await;

        match check_result {
            Ok(Some((user_id, pinned))) => {
                if user_id != auth_user.id && !auth_user.is_superuser() {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(ApiResponse::error("You can only reorder your own videos")),
                    );
                }
                if !pinned {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error(&format!("Video {} is not pinned", id))),
                    );
                }
            }
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error(&format!("Video {} not found", id))),
                );
            }
            Err(e) => {
                tracing::error!("Failed to check video ownership: {:?}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to verify video ownership")),
                );
            }
        }
    }

    // Update pin_order
    for (index, id) in payload.ordered_ids.iter().enumerate() {
        let new_order = (index + 1) as i32;
        let result = sqlx::query("UPDATE videos SET pin_order = ? WHERE id = ?")
            .bind(new_order)
            .bind(id)
            .execute(&mut *tx)
            .await;

        if let Err(e) = result {
            tracing::error!("Failed to update pin_order for video {}: {:?}", id, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!(
                    "Failed to update pin order for video {}",
                    id
                ))),
            );
        }
    }

    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {:?}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit pin order changes")),
        );
    }

    tracing::info!(
        user_id = auth_user.id,
        count = payload.ordered_ids.len(),
        "Successfully reordered pinned videos"
    );
    (
        StatusCode::OK,
        Json(ApiResponse::success(
            "Pin order updated successfully".to_string(),
        )),
    )
}

// DELETE /video/{id} — Delete video (original file, transcoded file, thumbnail, and DB record)
async fn delete_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE id = ?",
        VIDEO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only delete your own videos")),
                );
            }

            // Delete from database first
            let result = sqlx::query("DELETE FROM videos WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    // Delete original file
                    if let Err(e) =
                        delete_file(&state.config.storage_dir, &item.stored_path).await
                    {
                        tracing::warn!("Failed to delete original video file: {}", e);
                    }

                    // Delete transcoded file (if exists)
                    if let Some(transcoded) = &item.transcoded_path {
                        if let Err(e) =
                            delete_file(&state.config.storage_dir, transcoded).await
                        {
                            tracing::warn!("Failed to delete transcoded video file: {}", e);
                        }
                    }

                    // Delete thumbnail (if exists)
                    if let Some(thumb) = &item.thumbnail_path {
                        if let Err(e) =
                            delete_file(&state.config.storage_dir, thumb).await
                        {
                            tracing::warn!("Failed to delete video thumbnail: {}", e);
                        }
                    }

                    (
                        StatusCode::OK,
                        Json(ApiResponse::success("Video deleted".to_string())),
                    )
                }
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to delete video")),
                ),
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Video not found")),
        ),
    }
}

// POST /video/{id}/reprocess — Retry FFmpeg processing
async fn reprocess_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<VideoItem>>) {
    use uuid::Uuid;

    let item: Result<VideoItem, _> = sqlx::query_as(&format!(
        "SELECT {} FROM videos WHERE id = ?",
        VIDEO_COLUMNS
    ))
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let item = match item {
        Ok(item) => item,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(ApiResponse::error("Video not found")));
        }
    };

    if item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only reprocess your own videos")),
        );
    }

    // Verify raw file exists
    let full_path = std::path::PathBuf::from(&state.config.storage_dir).join(&item.stored_path);
    if tokio::fs::metadata(&full_path).await.is_err() {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Raw video file not found on disk. Cannot reprocess.")),
        );
    }

    // Set status to processing
    if let Err(e) = sqlx::query("UPDATE videos SET status = 'processing' WHERE id = ?")
        .bind(id)
        .execute(&state.db.pool)
        .await
    {
        tracing::error!("Failed to update status to processing: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to start reprocessing")),
        );
    }

    let batch_id = Uuid::new_v4();
    tracing::info!(%batch_id, item_id = id, "Queued video reprocessing (returning 202 Accepted)");

    let db_pool = state.db.pool.clone();
    let storage_dir = state.config.storage_dir.clone();
    let semaphore = state.video_semaphore.clone();
    let item_id = item.id;
    let stored_path = item.stored_path.clone();
    let extension = get_extension(&item.original_filename).unwrap_or_default();
    let filename = item.original_filename.clone();

    tokio::spawn(async move {
        tracing::info!(%batch_id, item_id, %filename, "Background video reprocessing started");

        let _permit = match semaphore.acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                tracing::error!(%batch_id, item_id, "Failed to acquire video processing semaphore");
                let _ = sqlx::query("UPDATE videos SET status = 'failed_processing' WHERE id = ?")
                    .bind(item_id)
                    .execute(&db_pool)
                    .await;
                return;
            }
        };

        let input_full_path = std::path::PathBuf::from(&storage_dir).join(&stored_path);
        let input_path_str = input_full_path.to_string_lossy().to_string();

        // Thumbnail
        let thumb_relative = generate_thumbnail_path(&stored_path);
        let thumb_full_path = std::path::PathBuf::from(&storage_dir).join(&thumb_relative);

        if let Some(parent) = thumb_full_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let final_thumb = match ffmpeg_extract_thumbnail(&input_path_str, &thumb_full_path.to_string_lossy()).await {
            Ok(_) => {
                tracing::info!(%batch_id, item_id, "Reprocess: thumbnail extracted");
                Some(thumb_relative)
            }
            Err(e) => {
                tracing::warn!(%batch_id, item_id, "Reprocess: thumbnail failed: {}", e);
                None
            }
        };

        // Transcoding
        let final_transcoded = if !is_web_safe_video(&extension) {
            let transcoded_relative = generate_transcoded_path(&stored_path);
            let transcoded_full_path = std::path::PathBuf::from(&storage_dir).join(&transcoded_relative);

            if let Some(parent) = transcoded_full_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            match ffmpeg_transcode_to_mp4(&input_path_str, &transcoded_full_path.to_string_lossy()).await {
                Ok(_) => {
                    tracing::info!(%batch_id, item_id, "Reprocess: transcoding successful");
                    Some(transcoded_relative)
                }
                Err(e) => {
                    tracing::error!(%batch_id, item_id, "Reprocess: transcoding failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        if final_thumb.is_some() || is_web_safe_video(&extension) {
            let _ = sqlx::query(
                "UPDATE videos SET status = 'active', thumbnail_path = ?, transcoded_path = ? WHERE id = ?"
            )
            .bind(&final_thumb)
            .bind(&final_transcoded)
            .bind(item_id)
            .execute(&db_pool)
            .await;
            tracing::info!(%batch_id, item_id, "Reprocess: video activated");
        } else {
            let _ = sqlx::query("UPDATE videos SET status = 'failed_processing' WHERE id = ?")
                .bind(item_id)
                .execute(&db_pool)
                .await;
            tracing::error!(%batch_id, item_id, "Reprocess: video processing failed");
        }

        tracing::info!(%batch_id, "Background video reprocessing completed");
    }.instrument(tracing::info_span!("video_bg_reprocess", %batch_id, item_id)));

    let mut response_item = item;
    response_item.status = "processing".to_string();

    (StatusCode::ACCEPTED, Json(ApiResponse::success(response_item)))
}
