use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::auth::{hash_password, verify_password};
use crate::models::{
    ApiResponse, LoginRequest, LoginResponse, RegisterRequest, User, UserResponse,
};
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/users", get(list_users))
        .route("/users/{id}", get(get_user).put(update_user).delete(delete_user))
}

// POST /api/register
async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Hash the password
    let password_hash = match hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to hash password")),
            );
        }
    };

    // Insert user into database
    let result = sqlx::query(
        "INSERT INTO users (name, username, email, password_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(&payload.name)
    .bind(&payload.username)
    .bind(&payload.email)
    .bind(&password_hash)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) => {
            let user_response = UserResponse {
                id: res.last_insert_id() as i32,
                name: payload.name,
                username: payload.username,
                email: payload.email,
            };
            (StatusCode::CREATED, Json(ApiResponse::success(user_response)))
        }
        Err(e) => {
            let error_msg = if e.to_string().contains("Duplicate entry") {
                "Username or email already exists"
            } else {
                "Failed to create user"
            };
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(error_msg)))
        }
    }
}

// POST /api/login
async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> (StatusCode, Json<ApiResponse<LoginResponse>>) {
    // Find user by username
    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash FROM users WHERE username = ?",
    )
    .bind(&payload.username)
    .fetch_one(&state.db.pool)
    .await;

    match user {
        Ok(user) => {
            // Verify password
            match verify_password(&payload.password, &user.password_hash) {
                Ok(true) => {
                    let response = LoginResponse {
                        message: "Login successful".to_string(),
                        user: user.into(),
                    };
                    (StatusCode::OK, Json(ApiResponse::success(response)))
                }
                _ => (
                    StatusCode::UNAUTHORIZED,
                    Json(ApiResponse::error("Invalid credentials")),
                ),
            }
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse::error("Invalid credentials")),
        ),
    }
}

// GET /api/users
async fn list_users(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<UserResponse>>>) {
    let users: Result<Vec<User>, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash FROM users",
    )
    .fetch_all(&state.db.pool)
    .await;

    match users {
        Ok(users) => {
            let user_responses: Vec<UserResponse> = users.into_iter().map(|u| u.into()).collect();
            (StatusCode::OK, Json(ApiResponse::success(user_responses)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to fetch users")),
        ),
    }
}

// GET /api/users/:id
async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    match user {
        Ok(user) => (StatusCode::OK, Json(ApiResponse::success(user.into()))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("User not found")),
        ),
    }
}

// PUT /api/users/:id
async fn update_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<RegisterRequest>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Hash the new password
    let password_hash = match hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to hash password")),
            );
        }
    };

    let result = sqlx::query(
        "UPDATE users SET name = ?, username = ?, email = ?, password_hash = ? WHERE id = ?",
    )
    .bind(&payload.name)
    .bind(&payload.username)
    .bind(&payload.email)
    .bind(&password_hash)
    .bind(id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let user_response = UserResponse {
                id,
                name: payload.name,
                username: payload.username,
                email: payload.email,
            };
            (StatusCode::OK, Json(ApiResponse::success(user_response)))
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("User not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update user")),
        ),
    }
}

// DELETE /api/users/:id
async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let result = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => (
            StatusCode::OK,
            Json(ApiResponse::success("User deleted".to_string())),
        ),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("User not found")),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to delete user")),
        ),
    }
}
