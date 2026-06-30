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
pub struct VideoItem {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    pub filename: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateVideoRequest {
    pub title: String,
    pub filename: String,
    pub url: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/video", get(list_videos).post(upload_video))
        .route("/video/{id}", get(get_video).delete(delete_video))
}

// GET /api/video - List videos (superuser sees all, others see only their own)
async fn list_videos(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<VideoItem>>>) {
    let items: Result<Vec<VideoItem>, _> = if auth_user.can_view_all_media() {
        // Superuser can see all videos
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM videos")
            .fetch_all(&state.db.pool)
            .await
    } else {
        // Others see only their own
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM videos WHERE user_id = ?")
            .bind(auth_user.id)
            .fetch_all(&state.db.pool)
            .await
    };

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch videos")),
        ),
    }
}

// POST /api/video - Upload video (all roles can upload)
async fn upload_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateVideoRequest>,
) -> (StatusCode, Json<ApiResponse<VideoItem>>) {
    let result = sqlx::query(
        "INSERT INTO videos (user_id, title, filename, url) VALUES (?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.title)
    .bind(&payload.filename)
    .bind(&payload.url)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = VideoItem {
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
            Json(ApiResponse::error("Failed to upload video")),
        ),
    }
}

// GET /api/video/:id - Get single video
async fn get_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<VideoItem>>) {
    let item: Result<VideoItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM videos WHERE id = ?",
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
                    Json(ApiResponse::error("You can only view your own videos")),
                );
            }
            (StatusCode::OK, Json(ApiResponse::success(item)))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Video not found")),
        ),
    }
}

// DELETE /api/video/:id - Delete video (owner or superuser)
async fn delete_video(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // First check ownership
    let item: Result<VideoItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM videos WHERE id = ?",
    )
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

            let result = sqlx::query("DELETE FROM videos WHERE id = ?")
                .bind(id)
                .execute(&state.db.pool)
                .await;

            match result {
                Ok(_) => (
                    StatusCode::OK,
                    Json(ApiResponse::success("Video deleted".to_string())),
                ),
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
