use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use std::sync::Arc;
use tower_cookies::{Cookie, Cookies};

use crate::auth::{create_token, hash_password, verify_password};
use crate::models::{
    ApiResponse, AuthUser, LoginRequest, LoginResponse, RegisterRequest, User, UserResponse,
};
use crate::AppState;

// Public routes (no authentication required)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
}

// Protected routes (authentication required) - middleware applied in main.rs
pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/me", get(get_current_user))
        .route("/users/{id}", get(get_user).put(update_user).delete(delete_user))
}

// POST /api/register
async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Validate input
    if payload.username.trim().is_empty() || payload.email.trim().is_empty() || payload.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("All fields are required")),
        );
    }

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

    // Insert user into database with default role 'user'
    let result = sqlx::query(
        "INSERT INTO users (name, username, email, password_hash, role) VALUES (?, ?, ?, ?, 'user')",
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
                role: "user".to_string(),
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
    cookies: Cookies,
    Json(payload): Json<LoginRequest>,
) -> (StatusCode, Json<ApiResponse<LoginResponse>>) {
    // Find user by username
    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE username = ?",
    )
    .bind(&payload.username)
    .fetch_one(&state.db.pool)
    .await;

    match user {
        Ok(user) => {
            // Verify password
            match verify_password(&payload.password, &user.password_hash) {
                Ok(true) => {
                    // Create JWT token
                    let token = match create_token(
                        user.id,
                        &user.username,
                        &user.role,
                        &state.config.jwt_secret,
                    ) {
                        Ok(t) => t,
                        Err(_) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ApiResponse::error("Failed to create token")),
                            );
                        }
                    };

                    // Set httpOnly cookie
                    let mut cookie = Cookie::new("auth_token", token.clone());
                    cookie.set_http_only(true);
                    cookie.set_secure(false); // Set to true in production (HTTPS only)
                    cookie.set_same_site(tower_cookies::cookie::SameSite::Lax); // Changed from None to Lax for localhost
                    cookie.set_path("/");
                    cookie.set_max_age(tower_cookies::cookie::time::Duration::days(365));
                    cookies.add(cookie);

                    let response = LoginResponse {
                        message: "Login successful".to_string(),
                        token,
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

// POST /api/logout
async fn logout(cookies: Cookies) -> (StatusCode, Json<ApiResponse<String>>) {
    // Clear the cookie by setting max_age to 0
    let mut cookie = Cookie::new("auth_token", "");
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
    cookies.add(cookie);

    (
        StatusCode::OK,
        Json(ApiResponse::success("Logged out successfully".to_string())),
    )
}

// GET /api/users/me - Get current authenticated user
async fn get_current_user(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
    )
    .bind(auth_user.id)
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

// GET /api/users - List users (superuser only)
async fn list_users(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ApiResponse<Vec<UserResponse>>>) {
    if !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only superuser can list all users")),
        );
    }

    let users: Result<Vec<User>, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users",
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
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Users can only view their own profile unless they are superuser
    if auth_user.id != id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only view your own profile")),
        );
    }

    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
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
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<RegisterRequest>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Users can only update their own profile unless they are superuser
    if auth_user.id != id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only update your own profile")),
        );
    }

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
            // Fetch the updated user to get the role
            let user: Result<User, _> = sqlx::query_as(
                "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&state.db.pool)
            .await;

            match user {
                Ok(user) => (StatusCode::OK, Json(ApiResponse::success(user.into()))),
                Err(_) => {
                    let user_response = UserResponse {
                        id,
                        name: payload.name,
                        username: payload.username,
                        email: payload.email,
                        role: auth_user.role.as_str().to_string(),
                    };
                    (StatusCode::OK, Json(ApiResponse::success(user_response)))
                }
            }
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
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Only superuser can delete users
    if !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only superuser can delete users")),
        );
    }

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
