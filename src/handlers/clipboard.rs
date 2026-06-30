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
pub struct ClipboardItem {
    pub id: i32,
    pub user_id: i32,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateClipboardRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateClipboardRequest {
    pub content: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/clipboard", get(list_clipboard).post(create_clipboard))
        .route("/clipboard/{id}", get(get_clipboard).put(update_clipboard).delete(delete_clipboard))
}

// GET /api/clipboard - List user's clipboard items (all roles, scoped to owner)
async fn list_clipboard(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<ClipboardItem>>>) {
    // All roles can access clipboard, but only their own
    let items: Result<Vec<ClipboardItem>, _> = sqlx::query_as(
        "SELECT id, user_id, content FROM clipboard WHERE user_id = ?",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match items {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::success(items))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch clipboard items")),
        ),
    }
}

// POST /api/clipboard - Create clipboard item (all roles)
async fn create_clipboard(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateClipboardRequest>,
) -> (StatusCode, Json<ApiResponse<ClipboardItem>>) {
    let result = sqlx::query(
        "INSERT INTO clipboard (user_id, content) VALUES (?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.content)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let item = ClipboardItem {
                id: res.last_insert_id() as i32,
                user_id: auth_user.id,
                content: payload.content,
            };
            (StatusCode::CREATED, Json(ApiResponse::success(item)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to create clipboard item")),
        ),
    }
}

// GET /api/clipboard/:id - Get single clipboard item (owner only)
async fn get_clipboard(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<ClipboardItem>>) {
    let item: Result<ClipboardItem, _> = sqlx::query_as(
        "SELECT id, user_id, content FROM clipboard WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(auth_user.id)
    .fetch_one(&state.db.pool)
    .await;

    match item {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::success(item))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Clipboard item not found")),
        ),
    }
}

// PUT /api/clipboard/:id - Update clipboard item (owner only)
async fn update_clipboard(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateClipboardRequest>,
) -> (StatusCode, Json<ApiResponse<ClipboardItem>>) {
    let result = sqlx::query(
        "UPDATE clipboard SET content = ? WHERE id = ? AND user_id = ?",
    )
    .bind(&payload.content)
    .bind(id)
    .bind(auth_user.id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let item = ClipboardItem {
                id,
                user_id: auth_user.id,
                content: payload.content,
            };
            (StatusCode::OK, Json(ApiResponse::success(item)))
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Clipboard item not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update clipboard item")),
        ),
    }
}

// DELETE /api/clipboard/:id - Delete clipboard item (owner only)
async fn delete_clipboard(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let result = sqlx::query("DELETE FROM clipboard WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(auth_user.id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => (
            StatusCode::OK,
            Json(ApiResponse::success("Clipboard item deleted".to_string())),
        ),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Clipboard item not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to delete clipboard item")),
        ),
    }
}
