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
pub struct AudioItem {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    pub filename: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateAudioRequest {
    pub title: String,
    pub filename: String,
    pub url: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/audio", get(list_audio).post(upload_audio))
        .route("/audio/{id}", get(get_audio).delete(delete_audio))
}

// GET /api/audio - List audio (superuser sees all, others see only their own)
async fn list_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<AudioItem>>>) {
    let items: Result<Vec<AudioItem>, _> = if auth_user.can_view_all_media() {
        // Superuser can see all audio
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM audio")
            .fetch_all(&state.db.pool)
            .await
    } else {
        // Others see only their own
        sqlx::query_as("SELECT id, user_id, title, filename, url FROM audio WHERE user_id = ?")
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

// POST /api/audio - Upload audio (all roles can upload)
async fn upload_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateAudioRequest>,
) -> (StatusCode, Json<ApiResponse<AudioItem>>) {
    let result = sqlx::query(
        "INSERT INTO audio (user_id, title, filename, url) VALUES (?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.title)
    .bind(&payload.filename)
    .bind(&payload.url)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = AudioItem {
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
            Json(ApiResponse::error("Failed to upload audio")),
        ),
    }
}

// GET /api/audio/:id - Get single audio
async fn get_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<AudioItem>>) {
    let item: Result<AudioItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM audio WHERE id = ?",
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
                    Json(ApiResponse::error("You can only view your own audio")),
                );
            }
            (StatusCode::OK, Json(ApiResponse::success(item)))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Audio not found")),
        ),
    }
}

// DELETE /api/audio/:id - Delete audio (owner or superuser)
async fn delete_audio(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // First check ownership
    let item: Result<AudioItem, _> = sqlx::query_as(
        "SELECT id, user_id, title, filename, url FROM audio WHERE id = ?",
    )
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
                Ok(_) => (
                    StatusCode::OK,
                    Json(ApiResponse::success("Audio deleted".to_string())),
                ),
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
