use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Serialize;
use sqlx::FromRow;
use std::sync::Arc;

use crate::media::{
    delete_file, generate_storage_path, read_file, save_file, validate_extension, validate_size,
    MediaType,
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
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery", get(list_gallery).post(upload_image))
        .route("/gallery/{id}", get(get_image).delete(delete_image))
        .route("/gallery/{id}/download", get(download_image))
}

// GET /api/gallery - List images (superuser sees all, others see only their own)
async fn list_gallery(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = if auth_user.can_view_all_media() {
        sqlx::query_as(
            "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type FROM gallery",
        )
        .fetch_all(&state.db.pool)
        .await
    } else {
        sqlx::query_as(
            "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type FROM gallery WHERE user_id = ?",
        )
        .bind(auth_user.id)
        .fetch_all(&state.db.pool)
        .await
    };

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch gallery items")),
        ),
    }
}

// POST /api/gallery - Upload image (multipart/form-data)
async fn upload_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let mut title: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut original_filename: Option<String> = None;

    // Parse multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "title" => {
                if let Ok(text) = field.text().await {
                    title = Some(text);
                }
            }
            "file" => {
                original_filename = field.file_name().map(|s| s.to_string());
                if let Ok(bytes) = field.bytes().await {
                    file_data = Some(bytes.to_vec());
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

    // Validate extension
    let extension = match validate_extension(MediaType::Gallery, &orig_filename) {
        Ok(ext) => ext,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(msg)));
        }
    };

    // Validate size (100 MB max for images)
    let size_bytes = file_bytes.len() as u64;
    if let Err(msg) = validate_size(MediaType::Gallery, size_bytes) {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiResponse::error(msg)),
        );
    }

    // Generate storage path
    let (stored_path, full_path) =
        generate_storage_path(&state.config.media_dir, MediaType::Gallery, &extension);

    // Save file to disk
    if let Err(e) = save_file(&full_path, &file_bytes).await {
        tracing::error!("Failed to save file: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to save file")),
        );
    }

    // Get MIME type
    let mime_type = MediaType::Gallery.mime_type_for_extension(&extension);

    // Insert into database
    let result = sqlx::query(
        "INSERT INTO gallery (user_id, title, original_filename, stored_path, size_bytes, mime_type) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&title)
    .bind(&orig_filename)
    .bind(&stored_path)
    .bind(size_bytes as i64)
    .bind(mime_type)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = GalleryItem {
                id: res.last_insert_id() as i32,
                user_id: auth_user.id,
                title,
                original_filename: orig_filename,
                stored_path,
                size_bytes: size_bytes as i64,
                mime_type: mime_type.to_string(),
            };
            (StatusCode::CREATED, Json(ApiResponse::success(item)))
        }
        Err(e) => {
            tracing::error!("Failed to insert gallery item: {}", e);
            // Try to clean up the saved file
            let _ = delete_file(&state.config.media_dir, &stored_path).await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to save image metadata")),
            )
        }
    }
}

// GET /api/gallery/:id - Get image metadata
async fn get_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            if item.user_id != auth_user.id && !auth_user.can_view_all_media() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::error("You can only view your own images")),
                );
            }
            (StatusCode::OK, Json(ApiResponse::success(item)))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Image not found")),
        ),
    }
}

// GET /api/gallery/:id/download - Download the actual image file
async fn download_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Response {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            if item.user_id != auth_user.id && !auth_user.can_view_all_media() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ApiResponse::<()>::error("You can only view your own images")),
                )
                    .into_response();
            }

            match read_file(&state.config.media_dir, &item.stored_path).await {
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

// DELETE /api/gallery/:id - Delete image (owner or superuser)
async fn delete_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, original_filename, stored_path, size_bytes, mime_type FROM gallery WHERE id = ?",
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
                    if let Err(e) = delete_file(&state.config.media_dir, &item.stored_path).await {
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
