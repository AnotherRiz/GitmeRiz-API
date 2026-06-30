use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;

use crate::models::{ApiResponse, AuthUser};
use crate::AppState;

#[derive(Debug, FromRow, Serialize)]
pub struct GalleryItem {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    pub filename: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateGalleryRequest {
    pub title: String,
    pub filename: String,
    pub url: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/gallery", get(list_gallery).post(upload_image))
        .route("/gallery/{id}", get(get_image).delete(delete_image))
}

// GET /api/gallery - List images (superuser sees all, others see only their own)
async fn list_gallery(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<GalleryItem>>>) {
    let items: Result<Vec<GalleryItem>, _> = if auth_user.can_view_all_media() {
        // Superuser can see all images
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM gallery")
            .fetch_all(&state.db.pool)
            .await
    } else {
        // Others see only their own
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM gallery WHERE user_id = ?")
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

// POST /api/gallery - Upload image (all roles can upload)
async fn upload_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateGalleryRequest>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let result = sqlx::query(
        "INSERT INTO gallery (user_id, title, filename, url) VALUES (?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.title)
    .bind(&payload.filename)
    .bind(&payload.url)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = GalleryItem {
                id: res.last_insert_id() as i32,
                user_id: auth_user.id,
                title: payload.title,
                filename: payload.filename,
                url: payload.url,
            };
            (StatusCode::CREATED, Json(ApiResponse::success(item)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to upload image")),
        ),
    }
}

// GET /api/gallery/:id - Get single image
async fn get_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<GalleryItem>>) {
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM gallery WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => {
            // Check permission: owner or superuser
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

// DELETE /api/gallery/:id - Delete image (owner or superuser)
async fn delete_image(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // First check ownership
    let item: Result<GalleryItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM gallery WHERE id = ?",
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

            let result = sqlx::query("DELETE FROM gallery WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => (
                    StatusCode::OK,
                    Json(ApiResponse::success("Image deleted".to_string())),
                ),
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
