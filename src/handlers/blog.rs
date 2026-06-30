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
pub struct BlogPost {
    pub id: i32,
    pub author_id: i32,
    pub title: String,
    pub content: String,
    pub published: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateBlogRequest {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub published: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBlogRequest {
    pub title: String,
    pub content: String,
    pub published: bool,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/blog", get(list_posts).post(create_post))
        .route("/blog/{id}", get(get_post).put(update_post).delete(delete_post))
}

// GET /api/blog - List blog posts (all roles can read)
async fn list_posts(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<BlogPost>>>) {
    // All authenticated users can read published posts
    let posts: Result<Vec<BlogPost>, _> = sqlx::query_as(
        "SELECT id, author_id, title, content, published FROM blog_posts WHERE published = true",
    )
    .fetch_all(&state.db.pool)
    .await;

    match posts {
        Ok(posts) => (StatusCode::OK, Json(ApiResponse::success(posts))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch blog posts")),
        ),
    }
}

// POST /api/blog - Create blog post (admin and superuser only)
async fn create_post(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateBlogRequest>,
) -> (StatusCode, Json<ApiResponse<BlogPost>>) {
    // Check permission: only admin and superuser can write
    if !auth_user.can_write_blog() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only admin and superuser can create blog posts")),
        );
    }

    let result = sqlx::query(
        "INSERT INTO blog_posts (author_id, title, content, published) VALUES (?, ?, ?, ?)",
    )
    .bind(auth_user.id)
    .bind(&payload.title)
    .bind(&payload.content)
    .bind(payload.published)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let post = BlogPost {
                id: res.last_insert_id() as i32,
                author_id: auth_user.id,
                title: payload.title,
                content: payload.content,
                published: payload.published,
            };
            (StatusCode::CREATED, Json(ApiResponse::success(post)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to create blog post")),
        ),
    }
}

// GET /api/blog/:id - Get single blog post (all roles can read published)
async fn get_post(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<BlogPost>>) {
    let post: Result<BlogPost, _> = sqlx::query_as(
        "SELECT id, author_id, title, content, published FROM blog_posts WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match post {
        Ok(post) => {
            // Unpublished posts can only be viewed by author or users who can write blog
            if !post.published && post.author_id != auth_user.id && !auth_user.can_write_blog() {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::error("Blog post not found")),
                );
            }
            (StatusCode::OK, Json(ApiResponse::success(post)))
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Blog post not found")),
        ),
    }
}

// PUT /api/blog/:id - Update blog post (admin and superuser only)
async fn update_post(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateBlogRequest>,
) -> (StatusCode, Json<ApiResponse<BlogPost>>) {
    // Check permission: only admin and superuser can write
    if !auth_user.can_write_blog() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only admin and superuser can update blog posts")),
        );
    }

    let result = sqlx::query(
        "UPDATE blog_posts SET title = ?, content = ?, published = ? WHERE id = ?",
    )
    .bind(&payload.title)
    .bind(&payload.content)
    .bind(payload.published)
    .bind(id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let post = BlogPost {
                id,
                author_id: auth_user.id,
                title: payload.title,
                content: payload.content,
                published: payload.published,
            };
            (StatusCode::OK, Json(ApiResponse::success(post)))
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Blog post not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update blog post")),
        ),
    }
}

// DELETE /api/blog/:id - Delete blog post (admin and superuser only)
async fn delete_post(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Check permission: only admin and superuser can delete
    if !auth_user.can_write_blog() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only admin and superuser can delete blog posts")),
        );
    }

    let result = sqlx::query("DELETE FROM blog_posts WHERE id = ?")
        .bind(id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => (
            StatusCode::OK,
            Json(ApiResponse::success("Blog post deleted".to_string())),
        ),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("Blog post not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to delete blog post")),
        ),
    }
}
