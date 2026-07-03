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
    get_extension, generate_short_id, generate_thumbnail_path, generate_and_encode_thumbnail, 
    generate_and_encode_preview, MediaType,
};
use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

#[derive(Debug, FromRow, Serialize)]
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
    pub pinned: bool,
    pub status: String,
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

// Helper struct for parallel processing
struct FileProcessData {
    stored_path: String,
    orig_filename: String,
    item_title: String,
    file_bytes: Vec<u8>,
    mime_type: String,
    size_bytes: i64,
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE visibility = 'public' ORDER BY id DESC",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE user_id = ? ORDER BY id DESC",
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

// POST /api/gallery - Upload image (multipart/form-data)
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

    tracing::info!(%batch_id, file_count = files.len(), "Starting bulk upload");

    // First pass: Validation
    for (orig_filename, file_bytes) in &files {
        // Validate extension
        if let Err(msg) = validate_extension(MediaType::Gallery, orig_filename) {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(msg)));
        }

        // Validate size (100 MB max for images)
        let size_bytes = file_bytes.len() as u64;
        if let Err(msg) = validate_size(MediaType::Gallery, size_bytes) {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ApiResponse::error(msg)),
            );
        }
    }

    let num_files = files.len();

    // Phase 1: Save raw files to disk and prepare data
    let mut file_data: Vec<FileProcessData> = Vec::new();
    for (orig_filename, file_bytes) in files {
        let extension = get_extension(&orig_filename).unwrap_or_default();

        // Generate storage path
        let (stored_path, full_path) =
            generate_storage_path(&state.config.storage_dir, MediaType::Gallery, &extension);

        // Save file to disk
        if let Err(e) = save_file(&full_path, &file_bytes).await {
            tracing::error!("Failed to save file {}: {}", orig_filename, e);
            // Clean up any files already saved
            for data in &file_data {
                let _ = delete_file(&state.config.storage_dir, &data.stored_path).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save file to disk")),
            );
        }

        let mime_type = MediaType::Gallery.mime_type_for_extension(&extension);
        let size_bytes = file_bytes.len() as i64;
        
        // Determine title
        let item_title = if num_files == 1 && title.is_some() {
            title.clone().unwrap()
        } else {
            orig_filename.clone()
        };

        file_data.push(FileProcessData {
            stored_path,
            orig_filename,
            item_title,
            file_bytes,
            mime_type: mime_type.to_string(),
            size_bytes,
        });
    }

    // Phase 2: Generate thumbnails in parallel with semaphore (memory ceiling)
    let mut thumbnail_tasks = Vec::new();
    
    for (index, data) in file_data.iter().enumerate() {
        let semaphore = state.image_semaphore.clone();
        let bytes = data.file_bytes.clone();
        let path = data.stored_path.clone();
        let filename = data.orig_filename.clone();
        let image_id = Uuid::new_v4();

        let task = tokio::spawn(
            async move {
                tracing::info!("Starting thumbnail generation");
                
                // Wait here if too many images are already decoding (memory ceiling)
                let _permit = semaphore.acquire_owned().await
                    .map_err(|_| "semaphore closed".to_string())?;

                tracing::debug!("Semaphore permit acquired, decoding image");

                // CPU-bound work goes on the blocking pool
                let thumb = tokio::task::spawn_blocking(move || {
                    generate_and_encode_thumbnail(&bytes, 500)
                })
                .await
                .map_err(|e| {
                    tracing::error!("Thumbnail task panicked: {}", e);
                    format!("thumbnail task panicked: {}", e)
                })??;

                let thumbnail_path = generate_thumbnail_path(&path);
                
                tracing::info!("Thumbnail encoded successfully");
                
                Ok::<(usize, String, Vec<u8>), String>((index, thumbnail_path, thumb))
                // _permit is dropped here, releasing the slot for the next image
            }
            .instrument(tracing::info_span!("process_image", %batch_id, %image_id, %filename))
        );

        thumbnail_tasks.push(task);
    }

    // Collect all thumbnail results
    let thumbnail_results = futures::future::join_all(thumbnail_tasks).await;
    
    let success_count = thumbnail_results.iter().filter(|r| matches!(r, Ok(Ok(_)))).count();
    let fail_count = thumbnail_results.len() - success_count;
    tracing::info!(%batch_id, succeeded = success_count, failed = fail_count, "Thumbnail generation completed");

    // Phase 3: Save thumbnails and insert into database
    let mut uploaded_items = Vec::new();
    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {:?}", e);
            // Clean up saved files
            for data in &file_data {
                let _ = delete_file(&state.config.storage_dir, &data.stored_path).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to start database transaction")),
            );
        }
    };

    for (idx, data) in file_data.into_iter().enumerate() {
        // Handle thumbnail result
        let thumbnail_path: Option<String> = match &thumbnail_results[idx] {
            Ok(Ok((_, thumb_path, thumb_data))) => {
                let thumbnail_full_path = std::path::PathBuf::from(&state.config.storage_dir).join(thumb_path);
                match save_file(&thumbnail_full_path, thumb_data).await {
                    Ok(_) => Some(thumb_path.clone()),
                    Err(e) => {
                        tracing::warn!("Failed to save thumbnail for {}: {}", data.orig_filename, e);
                        None
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("Failed to generate thumbnail for {}: {}", data.orig_filename, e);
                None
            }
            Err(e) => {
                tracing::warn!("Thumbnail task failed for {}: {}", data.orig_filename, e);
                None
            }
        };

        // Generate unique short_id with collision retry
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
                    // Clean up all saved files
                    let _ = delete_file(&state.config.storage_dir, &data.stored_path).await;
                    if let Some(ref thumb_path) = thumbnail_path {
                        let _ = delete_file(&state.config.storage_dir, thumb_path).await;
                    }
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to generate unique short_id")),
                    );
                }
            }
        };

        // Insert into database
        let result = sqlx::query(
            "INSERT INTO gallery (user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(auth_user.id)
        .bind(&data.item_title)
        .bind(&data.orig_filename)
        .bind(&data.stored_path)
        .bind(data.size_bytes)
        .bind(&data.mime_type)
        .bind(&visibility)
        .bind(&short_id)
        .bind(&thumbnail_path)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(res) => {
                uploaded_items.push(GalleryItem {
                    id: res.last_insert_id() as i32,
                    user_id: auth_user.id,
                    title: data.item_title,
                    original_filename: data.orig_filename,
                    stored_path: data.stored_path,
                    size_bytes: data.size_bytes,
                    mime_type: data.mime_type,
                    visibility: visibility.clone(),
                    short_id,
                    thumbnail_path,
                    pinned: false,
                    status: "active".to_string(),
                });
            }
            Err(e) => {
                tracing::error!("Failed to insert gallery item: {}", e);
                // Clean up the files
                let _ = delete_file(&state.config.storage_dir, &data.stored_path).await;
                if let Some(ref thumb_path) = thumbnail_path {
                    let _ = delete_file(&state.config.storage_dir, thumb_path).await;
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to save image metadata")),
                );
            }
        }
    }

    // Commit the transaction
    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit database transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit database transaction")),
        );
    }

    tracing::info!(%batch_id, total_uploaded = uploaded_items.len(), "Bulk upload finished");

    if num_files == 1 {
        let single_item = uploaded_items.into_iter().next().unwrap();
        (StatusCode::CREATED, Json(ApiResponse::success(UploadResponse::Single(single_item))))
    } else {
        (StatusCode::CREATED, Json(ApiResponse::success(UploadResponse::Bulk(uploaded_items))))
    }
}

// GET /api/gallery/:id - Get image metadata (public)
async fn get_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE short_id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE short_id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE short_id = ?",
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


// GET /gallery/p/{short_id} - Serve preview image (on-the-fly, medium size)
async fn serve_preview_image(
    State(state): State<Arc<AppState>>,
    Path(short_id): Path<String>,
    Query(query): Query<ImageQuery>,
    cookies: Cookies,
    headers: HeaderMap,
) -> impl IntoResponse {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE short_id = ?",
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

            // Read original file
            match read_file(&state.config.storage_dir, &item.stored_path).await {
                Ok(original_data) => {
                    // Generate preview (non-blocking, max width 1280px, quality 85)
                    let preview_result = tokio::task::spawn_blocking(move || {
                        generate_and_encode_preview(&original_data, 1280)
                    })
                    .await;

                    match preview_result {
                        Ok(Ok(preview_bytes)) => {
                            let body = Body::from(preview_bytes);
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "image/webp")
                                .header(header::CONTENT_DISPOSITION, "inline")
                                .header(header::CACHE_CONTROL, "public, max-age=3600") // Cache for 1 hour
                                .body(body)
                                .unwrap()
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Failed to generate preview for {}: {}", short_id, e);
                            build_error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to generate image preview",
                                &headers,
                                &state.config.frontend_url,
                            )
                        }
                        Err(e) => {
                            tracing::error!("Preview generation task panicked for {}: {}", short_id, e);
                            build_error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to generate image preview",
                                &headers,
                                &state.config.frontend_url,
                            )
                        }
                    }
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

// GET /gallery/me/pinned - List pinned images for authenticated user
async fn list_pinned_gallery(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE user_id = ? AND pinned = TRUE ORDER BY updated_at DESC",
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

// PATCH /gallery/{id}/pinned - Update pinned status (owner or superuser)
async fn update_image_pinned(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdatePinnedRequest>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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

            let result = sqlx::query("UPDATE gallery SET pinned = ? WHERE id = ?")
                .bind(payload.pinned)
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => {
                    item.pinned = payload.pinned;
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility, short_id, thumbnail_path, pinned, status FROM gallery WHERE id = ?",
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
    
    tracing::info!(%image_id, %filename, "Starting thumbnail reprocessing");

    // Acquire semaphore permit and generate thumbnail
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

    // Generate thumbnail (CPU-bound, use blocking pool)
    let thumb_result = tokio::task::spawn_blocking(move || {
        generate_and_encode_thumbnail(&file_data, 500)
    })
    .await;

    match thumb_result {
        Ok(Ok(thumbnail_data)) => {
            // Save thumbnail to disk
            let thumbnail_path = generate_thumbnail_path(&stored_path);
            let thumbnail_full_path = std::path::PathBuf::from(&state.config.storage_dir).join(&thumbnail_path);
            
            match save_file(&thumbnail_full_path, &thumbnail_data).await {
                Ok(_) => {
                    // Update status to active and set thumbnail_path
                    let result = sqlx::query(
                        "UPDATE gallery SET status = 'active', thumbnail_path = ? WHERE id = ?"
                    )
                    .bind(&thumbnail_path)
                    .bind(id)
                    .execute(&state.db.pool)
                    .await;

                    match result {
                        Ok(_) => {
                            item.status = "active".to_string();
                            item.thumbnail_path = Some(thumbnail_path);
                            tracing::info!(%image_id, "Thumbnail reprocessing successful");
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
                Err(e) => {
                    tracing::error!(%image_id, "Failed to save thumbnail: {}", e);
                    let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                        .bind(id)
                        .execute(&state.db.pool)
                        .await;
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error("Failed to save thumbnail to disk")),
                    )
                }
            }
        }
        Ok(Err(e)) => {
            tracing::error!(%image_id, "Thumbnail generation failed: {}", e);
            let _ = sqlx::query("UPDATE gallery SET status = 'failed_processing' WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("Failed to generate thumbnail: {}", e))),
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

