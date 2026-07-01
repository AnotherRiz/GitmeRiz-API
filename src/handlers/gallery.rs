use std::sync::Arc;
use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, Response, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, patch},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::media::{
    delete_file, generate_storage_path, read_file, save_file, validate_extension, validate_size,
    get_extension, MediaType,
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
struct UpdateVisibilityRequest {
    visibility: String,
}

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery", get(list_gallery))
        .route("/gallery/{id}", get(get_image))
        .route("/gallery/{id}/download", get(download_image))
}

pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery", post(upload_image))
        .route("/gallery/my", get(list_my_gallery))
        .route("/gallery/{id}", delete(delete_image))
        .route("/gallery/{id}/title", patch(update_image_title))
        .route("/gallery/{id}/visibility", patch(update_image_visibility))
}

// GET /api/gallery - List all public images
async fn list_gallery(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE visibility = 'public' ORDER BY id DESC",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE user_id = ? ORDER BY id DESC",
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

    let mut uploaded_items = Vec::new();
    let mut saved_paths: Vec<String> = Vec::new();

    // Start a transaction
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

    let num_files = files.len();

    for (orig_filename, file_bytes) in files {
        let extension = get_extension(&orig_filename).unwrap_or_default();

        // Generate storage path
        let (stored_path, full_path) =
            generate_storage_path(&state.config.storage_dir, MediaType::Gallery, &extension);

        // Save file to disk
        if let Err(e) = save_file(&full_path, &file_bytes).await {
            tracing::error!("Failed to save file: {}", e);
            // Clean up any files already saved to disk in this request
            for path in &saved_paths {
                let _ = delete_file(&state.config.storage_dir, path).await;
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save file to disk")),
            );
        }

        // Track saved path for cleanup
        saved_paths.push(stored_path.clone());

        // Get MIME type
        let mime_type = MediaType::Gallery.mime_type_for_extension(&extension);

        // Determine title: if single file and title is provided, use it. Otherwise use original filename.
        let item_title = if num_files == 1 && title.is_some() {
            title.clone().unwrap()
        } else {
            orig_filename.clone()
        };

        let size_bytes = file_bytes.len() as i64;

        // Insert into database
        let result = sqlx::query(
            "INSERT INTO gallery (user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(auth_user.id)
        .bind(&item_title)
        .bind(&orig_filename)
        .bind(&stored_path)
        .bind(size_bytes)
        .bind(mime_type)
        .bind(&visibility)
        .execute(&mut *tx)
        .await;

        match result {
            Ok(res) => {
                uploaded_items.push(GalleryItem {
                    id: res.last_insert_id() as i32,
                    user_id: auth_user.id,
                    title: item_title,
                    original_filename: orig_filename,
                    stored_path,
                    size_bytes,
                    mime_type: mime_type.to_string(),
                    visibility: visibility.clone(),
                });
            }
            Err(e) => {
                tracing::error!("Failed to insert gallery item: {}", e);
                // Clean up the files
                for path in &saved_paths {
                    let _ = delete_file(&state.config.storage_dir, path).await;
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
        // Clean up the files
        for path in &saved_paths {
            let _ = delete_file(&state.config.storage_dir, path).await;
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to commit database transaction")),
        );
    }

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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE id = ?",
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
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility FROM gallery WHERE id = ?",
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
                    // Delete file from disk
                    if let Err(e) = delete_file(&state.config.storage_dir, &item.stored_path).await {
                        tracing::warn!("Failed to delete file from disk: {}", e);
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
