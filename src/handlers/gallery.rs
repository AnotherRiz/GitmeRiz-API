use std::sync::Arc;
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, patch},
    Extension, Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_cookies::Cookies;
use tracing::Instrument;

use crate::auth::validate_token;
use crate::error_page::build_error_response;
use crate::media::{
    delete_file, generate_storage_path, read_file, save_file, validate_extension, validate_size,
    get_extension, generate_short_id, generate_thumbnail_path, generate_preview_path,
    generate_thumbnail_and_preview, MediaType,
};
use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct GalleryItem {
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
    pub preview_path: Option<String>,
    pub pinned: bool,
    pub status: String,
    pub pin_order: i32,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UploadResponse {
    Single(GalleryItem),
    Bulk(Vec<GalleryItem>),
}

#[derive(Debug, Deserialize)]
struct UpdateTitleRequest {
    title: String,
}

#[derive(Debug, Deserialize)]
struct UpdatePinnedRequest {
    pinned: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateVisibilityRequest {
    visibility: String,
}

#[derive(Debug, Deserialize)]
struct StatusCheckRequest {
    ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
struct ReorderPinsRequest {
    ordered_ids: Vec<i32>,
}

// Query parameters for image access with signed URL
#[derive(Debug, Deserialize)]
struct ImageQuery {
    /// Expiration timestamp (Unix timestamp)
    expires: Option<i64>,
    /// HMAC signature: SHA256(short_id + user_id + expires + secret)
    sig: Option<String>,
}

// Response for signed URL generation
#[derive(Debug, Serialize)]
struct SignedUrlResponse {
    url: String,
    expires_at: i64,
}

/// Generate HMAC-SHA256 signature for signed URL
fn generate_signature(short_id: &str, user_id: i32, expires: i64, secret: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    // Create a simple hash (in production, use proper HMAC-SHA256)
    let data = format!("{}:{}:{}:{}", short_id, user_id, expires, secret);
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    
    // Convert to hex string (16 chars)
    format!("{:016x}", hash)
}

/// Validate signed URL parameters
fn validate_signed_url(
    query: &ImageQuery,
    short_id: &str,
    owner_user_id: i32,
    secret: &str,
) -> Result<(), &'static str> {
    let expires = query.expires.ok_or("Missing 'expires' parameter")?;
    let sig = query.sig.as_ref().ok_or("Missing 'sig' parameter")?;
    
    // Check if URL has expired
    let now = Utc::now().timestamp();
    if now > expires {
        return Err("URL has expired");
    }
    
    // Validate signature
    let expected_sig = generate_signature(short_id, owner_user_id, expires, secret);
    if sig != &expected_sig {
        return Err("Invalid signature");
    }
    
    Ok(())
}

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery/public", get(list_gallery))
        .route("/gallery/{id}", get(get_image))
        .route("/gallery/d/{id}", get(download_image))
        .route("/gallery/r/{short_id}", get(serve_raw_image))
        .route("/gallery/t/{short_id}", get(serve_thumbnail_image))
        .route("/gallery/p/{short_id}", get(serve_preview_image))
}

pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery", post(upload_image))
        .route("/gallery/me", get(list_my_gallery))
        .route("/gallery/me/pinned", get(list_pinned_gallery))
        .route("/gallery/status", post(check_status))
        .route("/gallery/reorder-pins", patch(reorder_pins))
        .route("/gallery/{id}", delete(delete_image))
        .route("/gallery/{id}/title", patch(update_image_title))
        .route("/gallery/{id}/visibility", patch(update_image_visibility))
        .route("/gallery/{id}/pinned", patch(update_image_pinned))
        .route("/gallery/{id}/reprocess", post(reprocess_image))
        .route("/gallery/{short_id}/sign", post(generate_signed_url))
}

// Helper function to extract optional authentication from cookie or header
// (Query parameter now uses signed URLs instead of JWT token)
fn extract_optional_auth(
    cookies: &Cookies,
    headers: &HeaderMap,
    jwt_secret: &str,
) -> Option<AuthUser> {
    // Priority 1: Read from cookie (preferred for security)
    let from_cookie = cookies
        .get("auth_token")
        .and_then(|c| {
            tracing::debug!("Found auth_token cookie");
            validate_token(c.value(), jwt_secret).ok()
        });
    
    if from_cookie.is_some() {
        tracing::debug!("Authentication successful from cookie");
        return from_cookie;
    }
    
    // Priority 2: Fallback to Authorization header (for API clients)
    let from_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|auth_header| {
            if auth_header.starts_with("Bearer ") {
                let token = &auth_header[7..];
                tracing::debug!("Found Authorization header");
                validate_token(token, jwt_secret).ok()
            } else {
                None
            }
        });
    
    if from_header.is_some() {
        tracing::debug!("Authentication successful from header");
        return from_header;
    }
    
    tracing::debug!("No authentication found in cookie or header");
    None
}

// GET /api/gallery - List all public images
async fn list_gallery(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE visibility = 'public' ORDER BY id DESC",
    )
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(e) => {
            tracing::error!("Failed to fetch gallery items: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to fetch gallery items")),
            )
        }
    }
}

// GET /api/gallery/my - List all user-specific images (public & private)
async fn list_my_gallery(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE user_id = ? ORDER BY id DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(e) => {
            tracing::error!("Failed to fetch user gallery items: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to fetch gallery items")),
            )
        }
    }
}

// POST /api/gallery - Upload image with BACKGROUND PROCESSING (multipart/form-data)
// Returns 202 Accepted immediately after saving raw files
// Processing (thumbnail/preview generation) happens in detached background task
async fn upload_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse<UploadResponse>>) {
    use uuid::Uuid;
    
    let batch_id = Uuid::new_v4();
    
    let mut title: Option<String> = None;
    let mut visibility = "private".to_string();
    let mut files = Vec::new();

    // Parse multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
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
                let orig_filename = field.file_name().map(|s| s.to_string());
                if let Some(name) = orig_filename {
                    if !name.is_empty() {
                        if let Ok(bytes) = field.bytes().await {
                            files.push((name, bytes.to_vec()));
                            if files.len() > 50 {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(ApiResponse::error("Too many files. Limit is 50 files per upload.")),
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if files.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No file provided")),
        );
    }

    tracing::info!(%batch_id, file_count = files.len(), "Starting upload with background processing");

    // First pass: Validation
    for (orig_filename, file_bytes) in &files {
        if let Err(msg) = validate_extension(MediaType::Gallery, orig_filename) {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(msg)));
        }

        let size_bytes = file_bytes.len() as u64;
        if let Err(msg) = validate_size(MediaType::Gallery, size_bytes) {
            return (StatusCode::PAYLOAD_TOO_LARGE, Json(ApiResponse::error(msg)));
        }
    }

    let num_files = files.len();

    // ============================================================================
    // PHASE 1: FAST UPLOAD - Save raw files and insert DB (< 200ms)
    // ============================================================================
    
    let mut uploaded_items: Vec<GalleryItem> = Vec::new();
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

    for (orig_filename, file_bytes) in &files {
        let extension = get_extension(orig_filename).unwrap_or_default();
        let (stored_path, full_path) =
            generate_storage_path(&state.config.storage_dir, MediaType::Gallery, &extension);

        // Save raw file to disk
        if let Err(e) = save_file(&full_path, file_bytes).await {
            tracing::error!("Failed to save file {}: {}", orig_filename, e);
            for item in &uploaded_items {
                let _ = delete_file(&state.config.storage_dir, &item.stored_path).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save file to disk")),
            );
        }

        let mime_type = MediaType::Gallery.mime_type_for_extension(&extension);
        let size_bytes = file_bytes.len() as i64;
        let item_title = if num_files == 1 && title.is_some() {
            title.clone().unwrap()
        } else {
            orig_filename.clone()
        };

        // Generate unique short_id
        let short_id = loop {
            let candidate = generate_short_id();
            let exists: Result<Option<(i32,)>, _> = sqlx::query_as(
                "SELECT id FROM gallery WHERE short_id = ?"
            )
            .bind(&candidate)
            .fetch_optional(&mut *tx)
            .await;

            match exists {
                Ok(None) => break candidate,
                Ok(Some(_)) => continue,
                Err(e) => {
                    tracing::error!("Failed to check short_id uniqueness: {}", e);
                    for item in &uploaded_items {
                        let _ = delete_file(&state.config.storage_dir, &item.stored_path).await;
                    }
                    let _ = delete_file(&state.config.storage_dir, &stored_path).await;
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to generate unique short_id")),
                    );
                }
            }
        };

        // Insert with status='processing' (thumbnail/preview NULL)
        let result = sqlx::query(
            "INSERT INTO gallery (user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, status) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'processing')",
        )
        .bind(auth_user.id)
        .bind(&item_title)
        .bind(&orig_filename)
        .bind(&stored_path)
        .bind(size_bytes)
        .bind(&mime_type)
        .bind(&visibility)
        .bind(&short_id)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(res) => {
                uploaded_items.push(GalleryItem {
                    id: res.last_insert_id() as i32,
                    user_id: auth_user.id,
                    title: item_title,
                    original_filename: orig_filename.clone(),
                    stored_path,
                    size_bytes,
                    mime_type: mime_type.to_string(),
                    visibility: visibility.clone(),
                    short_id,
                    thumbnail_path: None,  // Generated in background
                    preview_path: None,    // Generated in background
                    pinned: false,
                    status: "processing".to_string(),
                    pin_order: 0,
                });
            }
            Err(e) => {
                tracing::error!("Failed to insert gallery item: {}", e);
                for item in &uploaded_items {
                    let _ = delete_file(&state.config.storage_dir, &item.stored_path).await;
                }
                let _ = delete_file(&state.config.storage_dir, &stored_path).await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to save image metadata")),
                );
            }
        }
    }

    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit database transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit database transaction")),
        );
    }

    tracing::info!(%batch_id, total_uploaded = uploaded_items.len(), "Raw files saved, spawning background processing");

    // ============================================================================
    // PHASE 2: SPAWN DETACHED BACKGROUND TASK (does NOT block HTTP response!)
    // ============================================================================
    
    let db_pool = state.db.pool.clone();
    let storage_dir = state.config.storage_dir.clone();
    let semaphore = state.image_semaphore.clone();
    let items_to_process = uploaded_items.clone();

    tokio::spawn(async move {
        tracing::info!(%batch_id, "Background processing started");
        
        let mut processing_tasks = Vec::new();

        for item in &items_to_process {
            let semaphore = semaphore.clone();
            let storage_dir = storage_dir.clone();
            let stored_path = item.stored_path.clone();
            let item_id = item.id;
            let image_id = Uuid::new_v4();
            let filename = item.original_filename.clone();

            let task = tokio::spawn(
                async move {
                    // Read raw file
                    let file_bytes = match read_file(&storage_dir, &stored_path).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            tracing::error!("Failed to read raw file: {}", e);
                            return Err(format!("Failed to read file: {}", e));
                        }
                    };

                    // Acquire semaphore (memory ceiling)
                    let _permit = semaphore.acquire_owned().await
                        .map_err(|_| "semaphore closed".to_string())?;

                    // Generate thumbnail + preview (single decode)
                    let result = tokio::task::spawn_blocking(move || {
                        generate_thumbnail_and_preview(&file_bytes)
                    })
                    .await
                    .map_err(|e| format!("spawn_blocking panicked: {}", e))?;

                    let thumbnail_path = generate_thumbnail_path(&stored_path);
                    let preview_path = generate_preview_path(&stored_path);
                    
                    match result {
                        Ok((thumb_bytes, preview_bytes)) => {
                            Ok((item_id, thumbnail_path, preview_path, thumb_bytes, preview_bytes))
                        }
                        Err(e) => Err(e)
                    }
                }
                .instrument(tracing::info_span!("bg_process", %batch_id, %image_id, %filename))
            );

            processing_tasks.push(task);
        }

        let processing_results = futures::future::join_all(processing_tasks).await;
        
        let success_count = processing_results.iter().filter(|r| matches!(r, Ok(Ok(_)))).count();
        let fail_count = processing_results.len() - success_count;
        tracing::info!(%batch_id, succeeded = success_count, failed = fail_count, "Processing completed");

        // Save files in parallel
        let mut save_tasks = Vec::new();
        
        for result in &processing_results {
            if let Ok(Ok((item_id, thumb_path, prev_path, thumb_bytes, preview_bytes))) = result {
                let storage_dir = storage_dir.clone();
                let thumb_path = thumb_path.clone();
                let prev_path = prev_path.clone();
                let thumb_bytes = thumb_bytes.clone();
                let preview_bytes = preview_bytes.clone();
                let item_id = *item_id;
                
                let save_task = tokio::spawn(async move {
                    let thumbnail_full_path = std::path::PathBuf::from(&storage_dir).join(&thumb_path);
                    let preview_full_path = std::path::PathBuf::from(&storage_dir).join(&prev_path);
                    
                    let thumb_result = save_file(&thumbnail_full_path, &thumb_bytes).await;
                    let preview_result = save_file(&preview_full_path, &preview_bytes).await;
                    
                    let final_thumb = if thumb_result.is_ok() { Some(thumb_path) } else { None };
                    let final_preview = if preview_result.is_ok() { Some(prev_path) } else { None };
                    
                    (item_id, final_thumb, final_preview)
                });
                
                save_tasks.push(save_task);
            }
        }
        
        let save_results = futures::future::join_all(save_tasks).await;
        
        // Update database
        for result in save_results {
            if let Ok((item_id, thumb_path, prev_path)) = result {
                if thumb_path.is_some() || prev_path.is_some() {
                    let _ = sqlx::query(
                        "UPDATE gallery SET status = 'active', thumbnail_path = ?, preview_path = ? WHERE id = ?"
                    )
                    .bind(&thumb_path)
                    .bind(&prev_path)
                    .bind(item_id)
                    .execute(&db_pool)
                    .await;
                    
                    tracing::info!(%batch_id, item_id, "Successfully activated");
                } else {
                    let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                        .bind(item_id)
                        .execute(&db_pool)
                        .await;
                    
                    tracing::error!(%batch_id, item_id, "Failed to save files");
                }
            }
        }

        // Mark failed processing
        for (idx, result) in processing_results.iter().enumerate() {
            if matches!(result, Ok(Err(_)) | Err(_)) {
                let item_id = items_to_process[idx].id;
                let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                    .bind(item_id)
                    .execute(&db_pool)
                    .await;
            }
        }
        
        tracing::info!(%batch_id, "Background processing fully completed");
    });

    // ============================================================================
    // PHASE 3: RETURN 202 ACCEPTED IMMEDIATELY (User doesn't wait!)
    // ============================================================================
    
    tracing::info!(%batch_id, "Returning 202 Accepted (processing continues in background)");

    if num_files == 1 {
        let single_item = uploaded_items.into_iter().next().unwrap();
        (StatusCode::ACCEPTED, Json(ApiResponse::success(UploadResponse::Single(single_item))))
    } else {
        (StatusCode::ACCEPTED, Json(ApiResponse::success(UploadResponse::Bulk(uploaded_items))))
    }
}

// GET /api/gallery/:id - Get image metadata (public)
async fn get_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::success(item))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// GET /api/gallery/:id/download - Download the actual image file (public)
async fn download_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
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
            Json(ApiResponse::<()>::error("Image not found")),
        )
            .into_response(),
    }
}

// PATCH /api/gallery/:id/title - Rename image (owner or superuser)
async fn update_image_title(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateTitleRequest>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    if payload.title.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Title cannot be empty")),
        );
    }

    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(mut item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only edit your own images")),
                );
            }

            let result = sqlx::query("UPDATE gallery SET title = ? WHERE id = ?")
                .bind(&payload.title)
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    item.title = payload.title;
                    (StatusCode::OK, Json(ApiResponse::success(item)))
                }
                Err(e) => {
                    tracing::error!("Failed to update image title: {:?}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to update image title")),
                    )
                }
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// PATCH /api/gallery/:id/visibility - Change visibility (owner or superuser)
async fn update_image_visibility(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateVisibilityRequest>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let val = payload.visibility.trim().to_lowercase();
    if val != "public" && val != "private" {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Visibility must be 'public' or 'private'")),
        );
    }

    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(mut item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only edit your own images")),
                );
            }

            let result = sqlx::query("UPDATE gallery SET visibility = ? WHERE id = ?")
                .bind(&val)
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    item.visibility = val;
                    (StatusCode::OK, Json(ApiResponse::success(item)))
                }
                Err(e) => {
                    tracing::error!("Failed to update image visibility: {:?}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to update image visibility")),
                    )
                }
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// DELETE /api/gallery/:id - Delete image (owner or superuser)
async fn delete_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only delete your own images")),
                );
            }

            // Delete from database first
            let result = sqlx::query("DELETE FROM gallery WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    // Delete original file from disk
                    if let Err(e) = delete_file(&state.config.storage_dir, &item.stored_path).await {
                        tracing::warn!("Failed to delete file from disk: {}", e);
                    }
                    
                    // Delete thumbnail from disk (if exists)
                    if let Some(thumb_path) = &item.thumbnail_path {
                        if let Err(e) = delete_file(&state.config.storage_dir, thumb_path).await {
                            tracing::warn!("Failed to delete thumbnail from disk: {}", e);
                        }
                    }
                    
                    // Delete preview from disk (if exists)
                    if let Some(preview_path) = &item.preview_path {
                        if let Err(e) = delete_file(&state.config.storage_dir, preview_path).await {
                            tracing::warn!("Failed to delete preview from disk: {}", e);
                        }
                    }
                    
                    (
                        StatusCode::OK,
                        Json(ApiResponse::success("Image deleted".to_string())),
                    )
                }
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to delete image")),
                ),
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// GET /api/gallery/r/{short_id} - Serve raw image file (inline, for browser/img tag)
async fn serve_raw_image(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Query(query): Query<ImageQuery>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE short_id = ?",
    )
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control based on visibility:
            // - Public images: accessible to everyone
            // - Private images: require authentication OR valid signed URL
            if item.visibility == "private" {
                // Method 1: Try signed URL (for <img> tags)
                if query.expires.is_some() && query.sig.is_some() {
                    match validate_signed_url(&query, &short_id, item.user_id, &state.config.jwt_secret) {
                        Ok(()) => {
                            tracing::debug!("Access granted via signed URL");
                            // Signed URL valid, continue to serve
                        }
                        Err(e) => {
                            tracing::warn!("Signed URL validation failed: {}", e);
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                e,
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                } else {
                    // Method 2: Try cookie/header authentication
                    let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                    
                    match auth_user {
                        Some(user) => {
                            // Check if user is owner or superuser
                            if item.user_id != user.id && !user.is_superuser() {
                                return build_error_response(
                                    StatusCode::FORBIDDEN,
                                    "You can only access your own private images",
                                    &headers,
                                    &state.config.frontend_url,
                                );
                            }
                            // Access granted, continue to serve file
                        }
                        None => {
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                "This image is private. Authentication required.",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
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
                            format!("inline; filename=\"{}\"", item.original_filename),
                        )
                        .body(body)
                        .unwrap()
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
            "Image not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /api/gallery/t/{short_id} - Serve thumbnail image (inline, for browser/img tag)
async fn serve_thumbnail_image(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Query(query): Query<ImageQuery>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE short_id = ?",
    )
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control based on visibility:
            // - Public images: accessible to everyone
            // - Private images: require authentication OR valid signed URL
            if item.visibility == "private" {
                // Method 1: Try signed URL (for <img> tags)
                if query.expires.is_some() && query.sig.is_some() {
                    match validate_signed_url(&query, &short_id, item.user_id, &state.config.jwt_secret) {
                        Ok(()) => {
                            tracing::debug!("Access granted via signed URL");
                            // Signed URL valid, continue to serve
                        }
                        Err(e) => {
                            tracing::warn!("Signed URL validation failed: {}", e);
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                e,
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                } else {
                    // Method 2: Try cookie/header authentication
                    let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                    
                    match auth_user {
                        Some(user) => {
                            // Check if user is owner or superuser
                            if item.user_id != user.id && !user.is_superuser() {
                                return build_error_response(
                                    StatusCode::FORBIDDEN,
                                    "You can only access your own private images",
                                    &headers,
                                    &state.config.frontend_url,
                                );
                            }
                            // Access granted, continue to serve thumbnail
                        }
                        None => {
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                "This image is private. Authentication required.",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                }
            }

            // Check if thumbnail exists, otherwise serve original (for backward compatibility)
            let file_path = if let Some(thumb_path) = &item.thumbnail_path {
                thumb_path
            } else {
                &item.stored_path
            };

            match read_file(&state.config.storage_dir, file_path).await {
                Ok(data) => {
                    let body = Body::from(data);
                    let content_type = if item.thumbnail_path.is_some() {
                        "image/webp" // Pre-generated thumbnails are WebP
                    } else {
                        &item.mime_type // Fallback to original mime type
                    };
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, content_type)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("inline; filename=\"thumb_{}\"", item.original_filename),
                        )
                        .header(header::CACHE_CONTROL, "public, max-age=31536000") // Cache for 1 year
                        .body(body)
                        .unwrap()
                }
                Err(_) => build_error_response(
                    StatusCode::NOT_FOUND,
                    "Thumbnail not found on disk",
                    &headers,
                    &state.config.frontend_url,
                ),
            }
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Image not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// POST /api/gallery/{short_id}/sign - Generate signed URL for private image (authenticated)
async fn generate_signed_url(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
) -> (StatusCode, Json<ApiResponse<SignedUrlResponse>>) {
    // Fetch the image
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE short_id = ?",
    )
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Check ownership (owner or superuser can generate signed URLs)
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only generate signed URLs for your own images")),
                );
            }

            // Generate expiration timestamp (15 minutes from now)
            let expires = Utc::now().timestamp() + 15 * 60;

            // Generate signature
            let sig = generate_signature(&short_id, item.user_id, expires, &state.config.jwt_secret);

            // Build signed URL (no /api prefix since routes are at root level)
            let base_url = format!("http://{}:{}/gallery", state.config.server_host, state.config.server_port);
            let raw_url = format!("{}/r/{}?expires={}&sig={}", base_url, short_id, expires, sig);

            (
                StatusCode::OK,
                Json(ApiResponse::success(SignedUrlResponse {
                    url: raw_url,
                    expires_at: expires,
                })),
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}


// GET /gallery/p/{short_id} - Serve preview image (pre-generated, medium size)
async fn serve_preview_image(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Query(query): Query<ImageQuery>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE short_id = ?",
    )
    .bind(&short_id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Access control (same as raw and thumbnail)
            if item.visibility == "private" {
                if query.expires.is_some() && query.sig.is_some() {
                    match validate_signed_url(&query, &short_id, item.user_id, &state.config.jwt_secret) {
                        Ok(()) => {},
                        Err(e) => {
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                e,
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                } else {
                    let auth_user = extract_optional_auth(&cookies, &headers, &state.config.jwt_secret);
                    match auth_user {
                        Some(user) => {
                            if item.user_id != user.id && !user.is_superuser() {
                                return build_error_response(
                                    StatusCode::FORBIDDEN,
                                    "You can only access your own private images",
                                    &headers,
                                    &state.config.frontend_url,
                                );
                            }
                        }
                        None => {
                            return build_error_response(
                                StatusCode::UNAUTHORIZED,
                                "This image is private. Authentication required.",
                                &headers,
                                &state.config.frontend_url,
                            );
                        }
                    }
                }
            }

            // Check if preview exists, otherwise fallback to original
            let file_path = if let Some(preview_path) = &item.preview_path {
                preview_path
            } else {
                &item.stored_path
            };

            match read_file(&state.config.storage_dir, file_path).await {
                Ok(data) => {
                    let body = Body::from(data);
                    let content_type = if item.preview_path.is_some() {
                        "image/webp" // Pre-generated previews are WebP
                    } else {
                        &item.mime_type // Fallback to original mime type
                    };
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, content_type)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("inline; filename=\"preview_{}\"", item.original_filename),
                        )
                        .header(header::CACHE_CONTROL, "public, max-age=3600") // Cache for 1 hour
                        .body(body)
                        .unwrap()
                }
                Err(_) => build_error_response(
                    StatusCode::NOT_FOUND,
                    "Preview not found on disk",
                    &headers,
                    &state.config.frontend_url,
                ),
            }
        }
        Err(_) => build_error_response(
            StatusCode::NOT_FOUND,
            "Image not found",
            &headers,
            &state.config.frontend_url,
        ),
    }
}

// GET /gallery/me/pinned - List pinned images for authenticated user
async fn list_pinned_gallery(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE user_id = ? AND pinned = TRUE ORDER BY pin_order ASC, updated_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(e) => {
            tracing::error!("Failed to fetch pinned gallery items: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to fetch pinned gallery items")),
            )
        }
    }
}

// POST /gallery/status - Check status of multiple images
async fn check_status(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StatusCheckRequest>,
) -> impl IntoResponse {
    use std::collections::HashMap;
    
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
    
    // Build SQL query with IN clause
    let placeholders = payload.ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query = format!(
        "SELECT id, status FROM gallery WHERE id IN ({}) AND user_id = ?",
        placeholders
    );
    
    let mut query_builder = sqlx::query_as::<_, (i32, String)>(&query);
    for id in &payload.ids {
        query_builder = query_builder.bind(id);
    }
    query_builder = query_builder.bind(auth_user.id);
    
    let results = query_builder.fetch_all(&state.db.pool).await;
    
    match results {
        Ok(rows) => {
            let mut status_map = HashMap::new();
            for (id, status) in rows {
                status_map.insert(id, status);
            }
            
            // For IDs not found (either doesn't exist or not owned by user), return null/not found
            // Client can detect missing IDs if needed
            
            (StatusCode::OK, Json(ApiResponse::success(status_map)))
        }
        Err(e) => {
            tracing::error!("Failed to check status: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<HashMap<i32, String>>::error("Failed to check status")),
            )
        }
    }
}

// PATCH /gallery/{id}/pinned - Update pinned status (owner or superuser)
async fn update_image_pinned(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdatePinnedRequest>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    const MAX_PINNED_IMAGES: i64 = 8;
    
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(mut item) => {
            if item.user_id != auth_user.id && !auth_user.is_superuser() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only edit your own images")),
                );
            }

            // If pinning, check the limit and assign pin_order
            if payload.pinned && !item.pinned {
                // Count existing pinned images
                let count_result: Result<(i64,), _> = sqlx::query_as(
                    "SELECT COUNT(*) FROM gallery WHERE user_id = ? AND pinned = TRUE"
                )
                .bind(auth_user.id)
                .fetch_one(&state.db.pool)
                .await;

                match count_result {
                    Ok((count,)) => {
                        if count >= MAX_PINNED_IMAGES {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(ApiResponse::error(&format!("You can only pin up to {} images. Please unpin another image first.", MAX_PINNED_IMAGES))),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to count pinned images: {:?}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to check pinned count")),
                        );
                    }
                }

                // Get max pin_order and assign next value
                let max_order_result: Result<Option<(i32,)>, _> = sqlx::query_as(
                    "SELECT MAX(pin_order) FROM gallery WHERE user_id = ? AND pinned = TRUE"
                )
                .bind(auth_user.id)
                .fetch_optional(&state.db.pool)
                .await;

                let new_pin_order = match max_order_result {
                    Ok(Some((max_order,))) => max_order + 1,
                    Ok(None) => 1,
                    Err(e) => {
                        tracing::error!("Failed to get max pin_order: {:?}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to assign pin order")),
                        );
                    }
                };

                let result = sqlx::query("UPDATE gallery SET pinned = TRUE, pin_order = ? WHERE id = ?")
                    .bind(new_pin_order)
                    .bind(id)
                    .execute(&state.db.pool)
                    .await;

                match result {
                    Ok(_) => {
                        item.pinned = true;
                        item.pin_order = new_pin_order;
                        (StatusCode::OK, Json(ApiResponse::success(item)))
                    }
                    Err(e) => {
                        tracing::error!("Failed to update image pinned status: {:?}", e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to update pinned status")),
                        )
                    }
                }
            } else if !payload.pinned && item.pinned {
                // Unpinning: reset pin_order to 0
                let result = sqlx::query("UPDATE gallery SET pinned = FALSE, pin_order = 0 WHERE id = ?")
                    .bind(id)
                    .execute(&state.db.pool)
                    .await;

                match result {
                    Ok(_) => {
                        item.pinned = false;
                        item.pin_order = 0;
                        (StatusCode::OK, Json(ApiResponse::success(item)))
                    }
                    Err(e) => {
                        tracing::error!("Failed to update image pinned status: {:?}", e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to update pinned status")),
                        )
                    }
                }
            } else {
                // No change needed
                (StatusCode::OK, Json(ApiResponse::success(item)))
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// POST /gallery/{id}/reprocess - Retry thumbnail generation for a failed image
async fn reprocess_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    use uuid::Uuid;
    
    // Fetch the item by id
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, preview_path, pinned, status, pin_order FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let mut item = match item {
        Ok(item) => item,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Image not found")),
            );
        }
    };

    // Check ownership
    if item.user_id != auth_user.id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only reprocess your own images")),
        );
    }

    // Read the raw file from disk
    let file_data = match read_file(&state.config.storage_dir, &item.stored_path).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Raw file not found for reprocessing: {}", e);
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Raw file not found on disk. Cannot reprocess.")),
            );
        }
    };

    // Set status to processing
    let _ = sqlx::query("UPDATE gallery SET status = 'processing' WHERE id = ?")
        .bind(id)
        .execute(&state.db.pool)
        .await;

    let image_id = Uuid::new_v4();
    let filename = item.original_filename.clone();
    let stored_path = item.stored_path.clone();
    
    tracing::info!(%image_id, %filename, "Starting thumbnail and preview reprocessing");

    // Acquire semaphore permit and generate thumbnail + preview
    let semaphore = state.image_semaphore.clone();
    let _permit = match semaphore.acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to acquire processing slot")),
            );
        }
    };

    // Generate thumbnail and preview (CPU-bound, use blocking pool)
    // Uses optimized single decode + cascading resize
    let process_result = tokio::task::spawn_blocking(move || {
        generate_thumbnail_and_preview(&file_data)
    })
    .await;

    match process_result {
        Ok(Ok((thumbnail_data, preview_data))) => {
            // Save both thumbnail and preview to disk in parallel
            let thumbnail_path = generate_thumbnail_path(&stored_path);
            let preview_path = generate_preview_path(&stored_path);
            let thumbnail_full_path = std::path::PathBuf::from(&state.config.storage_dir).join(&thumbnail_path);
            let preview_full_path = std::path::PathBuf::from(&state.config.storage_dir).join(&preview_path);
            
            let thumb_save = save_file(&thumbnail_full_path, &thumbnail_data);
            let preview_save = save_file(&preview_full_path, &preview_data);
            
            let (thumb_result, preview_result) = tokio::join!(thumb_save, preview_save);
            
            let mut save_errors = Vec::new();
            let mut final_thumb_path = None;
            let mut final_preview_path = None;
            
            match thumb_result {
                Ok(_) => final_thumb_path = Some(thumbnail_path),
                Err(e) => save_errors.push(format!("thumbnail: {}", e)),
            }
            
            match preview_result {
                Ok(_) => final_preview_path = Some(preview_path),
                Err(e) => save_errors.push(format!("preview: {}", e)),
            }
            
            if !save_errors.is_empty() {
                tracing::error!(%image_id, "Failed to save files: {}", save_errors.join(", "));
                let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                    .bind(id)
                    .execute(&state.db.pool)
                    .await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error(&format!("Failed to save files: {}", save_errors.join(", ")))),
                );
            }

            // Update status to active and set thumbnail_path + preview_path
            let result = sqlx::query(
                "UPDATE gallery SET status = 'active', thumbnail_path = ?, preview_path = ? WHERE id = ?"
            )
            .bind(&final_thumb_path)
            .bind(&final_preview_path)
            .bind(id)
            .execute(&state.db.pool)
            .await;

            match result {
                Ok(_) => {
                    item.status = "active".to_string();
                    item.thumbnail_path = final_thumb_path;
                    item.preview_path = final_preview_path;
                    tracing::info!(%image_id, "Thumbnail and preview reprocessing successful");
                    (StatusCode::OK, Json(ApiResponse::success(item)))
                }
                Err(e) => {
                    tracing::error!("Failed to update database after reprocessing: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to update database")),
                    )
                }
            }
        }
        Ok(Err(e)) => {
            tracing::error!(%image_id, "Image processing failed: {}", e);
            let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("Failed to generate thumbnail and preview: {}", e))),
            )
        }
        Err(e) => {
            tracing::error!(%image_id, "Thumbnail task panicked: {}", e);
            let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Thumbnail generation task panicked")),
            )
        }
    }
}


// PATCH /gallery/reorder-pins - Reorder pinned images (owner or superuser)
async fn reorder_pins(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReorderPinsRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    if payload.ordered_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No image IDs provided")),
        );
    }

    if payload.ordered_ids.len() > 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Cannot reorder more than 8 pinned images")),
        );
    }

    // Start transaction for atomic updates
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

    // Verify all images exist and belong to user
    for id in &payload.ordered_ids {
        let check_result: Result<Option<(i32, bool)>, _> = sqlx::query_as(
            "SELECT user_id, pinned FROM gallery WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await;

        match check_result {
            Ok(Some((user_id, pinned))) => {
                if user_id != auth_user.id && !auth_user.is_superuser() {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(ApiResponse::error("You can only reorder your own images")),
                    );
                }
                if !pinned {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error(&format!("Image {} is not pinned", id))),
                    );
                }
            }
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error(&format!("Image {} not found", id))),
                );
            }
            Err(e) => {
                tracing::error!("Failed to check image ownership: {:?}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to verify image ownership")),
                );
            }
        }
    }

    // Update pin_order for each image based on array position
    for (index, id) in payload.ordered_ids.iter().enumerate() {
        let new_order = (index + 1) as i32; // Start from 1
        let result = sqlx::query("UPDATE gallery SET pin_order = ? WHERE id = ?")
            .bind(new_order)
            .bind(id)
            .execute(&mut *tx)
            .await;

        if let Err(e) = result {
            tracing::error!("Failed to update pin_order for image {}: {:?}", id, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("Failed to update pin order for image {}", id))),
            );
        }
    }

    // Commit transaction
    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {:?}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit pin order changes")),
        );
    }

    tracing::info!(user_id = auth_user.id, count = payload.ordered_ids.len(), "Successfully reordered pinned images");
    (
        StatusCode::OK,
        Json(ApiResponse::success("Pin order updated successfully".to_string())),
    )
}
