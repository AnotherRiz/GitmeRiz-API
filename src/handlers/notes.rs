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
pub struct Note {
    pub id: i32,
    pub user_id: i32,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: String,
    pub content: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/notes", get(list_notes).post(create_note))
        .route("/notes/{id}", get(get_note).put(update_note).delete(delete_note))
}

// GET /api/notes - List user's notes (all roles, scoped to owner)
async fn list_notes(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<Note>>>) {
    // All roles can access notes, but only their own
    let notes: Result<Vec<Note>, _> = sqlx::query_as(
        "SELECT id, user_id, title, content FROM notes WHERE user_id = ?",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db.pool)
    .await;

    match notes {
        Ok(notes) => (StatusCode::OK, Json(ApiResponse::success(notes))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch notes")),
        ),
    }
}

// POST /api/notes - Create note (all roles)
async fn create_note(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateNoteRequest>,
) -> (StatusCode, Json<ApiResponse<Note>>) {
    let result = sqlx::query(
        "INSERT INTO notes (user_id, title, content) VALUES (?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.title)
    .bind(&payload.content)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let note = Note {
                id: res.last_insert_id() as i32,
                user_id: auth_user.id,
                title: payload.title,
                content: payload.content,
            };
            (StatusCode::CREATED, Json(ApiResponse::success(note)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to create note")),
        ),
    }
}

// GET /api/notes/:id - Get single note (owner only)
async fn get_note(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<Note>>) {
    let note: Result<Note, _> = sqlx::query_as(
        "SELECT id, user_id, title, content FROM notes WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(auth_user.id)
    .fetch_one(&state.db.pool)
    .await;

    match note {
        Ok(note) => (StatusCode::OK, Json(ApiResponse::success(note))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Note not found")),
        ),
    }
}

// PUT /api/notes/:id - Update note (owner only)
async fn update_note(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateNoteRequest>,
) -> (StatusCode, Json<ApiResponse<Note>>) {
    let result = sqlx::query(
        "UPDATE notes SET title = ?, content = ? WHERE id = ? AND user_id = ?",
    )
    .bind(&payload.title)
    .bind(&payload.content)
    .bind(id)
    .bind(auth_user.id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let note = Note {
                id,
                user_id: auth_user.id,
                title: payload.title,
                content: payload.content,
            };
            (StatusCode::OK, Json(ApiResponse::success(note)))
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Note not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update note")),
        ),
    }
}

// DELETE /api/notes/:id - Delete note (owner only)
async fn delete_note(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let result = sqlx::query("DELETE FROM notes WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(auth_user.id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => (
            StatusCode::OK,
            Json(ApiResponse::success("Note deleted".to_string())),
        ),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Note not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to delete note")),
        ),
    }
}
