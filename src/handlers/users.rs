use axum::{
    extract::{Path, Query, State, ConnectInfo},
    http::{StatusCode, HeaderMap},
    routing::{get, patch, post},
    Extension, Json, Router,
};
use std::sync::Arc;
use std::net::SocketAddr;
use tower_cookies::{Cookie, Cookies};

use crate::auth::{create_token, generate_refresh_token, hash_password, verify_password};
use crate::models::{
    ApiResponse, AuthUser, ChangePasswordRequest, LoginRequest, LoginResponse, 
    PaginatedUsersResponse, PaginationQuery, RegisterRequest, UpdateUserRequest, 
    User, UserResponse,
};
use crate::validation::{validate_email, validate_name, validate_password, validate_username};
use crate::AppState;

// Public routes (no authentication required)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/refresh", post(refresh))
}

// Protected routes (authentication required) - middleware applied in main.rs
pub fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/me", get(get_current_user))
        .route("/users/me/password", patch(change_password))
        .route("/users/{id}", get(get_user).patch(update_user).delete(delete_user))
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

    // Validate username
    if let Err(e) = validate_username(&payload.username) {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
    }

    // Validate name
    if let Err(e) = validate_name(&payload.name) {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
    }

    // Validate email
    if let Err(e) = validate_email(&payload.email) {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
    }

    // Validate password
    if let Err(e) = validate_password(&payload.password) {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
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
                    // Create JWT Access Token
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
                                Json(ApiResponse::error("Failed to create access token")),
                            );
                        }
                    };

                    // Generate Refresh Token
                    let refresh_token = generate_refresh_token();
                    
                    // Extract client IP and User Agent
                    let user_agent = headers
                        .get("user-agent")
                        .and_then(|h| h.to_str().ok())
                        .map(|s| s.to_string());
                    
                    let ip_address = headers
                        .get("x-forwarded-for")
                        .and_then(|h| h.to_str().ok())
                        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
                        .unwrap_or_else(|| addr.ip().to_string());

                    // Session ID and Expiration
                    let session_id = uuid::Uuid::new_v4().to_string();
                    let expires_at = chrono::Utc::now() + chrono::Duration::days(365);

                    // Insert session to DB with SHA-256 hash of refresh token
                    let session_insert = sqlx::query(
                        "INSERT INTO sessions (id, user_id, refresh_token, user_agent, ip_address, expires_at) \
                         VALUES (?, ?, SHA2(?, 256), ?, ?, ?)"
                    )
                    .bind(&session_id)
                    .bind(user.id)
                    .bind(&refresh_token)
                    .bind(user_agent)
                    .bind(ip_address)
                    .bind(expires_at)
                    .execute(&state.db.pool)
                    .await;

                    if let Err(e) = session_insert {
                        tracing::error!("Failed to record session in database: {}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error("Failed to establish session")),
                        );
                    }

                    // Set httpOnly cookie for Access Token (auth_token)
                    let mut auth_cookie = Cookie::new("auth_token", token.clone());
                    auth_cookie.set_http_only(true);
                    auth_cookie.set_secure(false); // Set to true in production (HTTPS only)
                    auth_cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
                    auth_cookie.set_path("/");
                    auth_cookie.set_max_age(tower_cookies::cookie::time::Duration::minutes(15));
                    cookies.add(auth_cookie);

                    // Set httpOnly cookie for Refresh Token (refresh_token)
                    let mut refresh_cookie = Cookie::new("refresh_token", refresh_token.clone());
                    refresh_cookie.set_http_only(true);
                    refresh_cookie.set_secure(false); // Set to true in production (HTTPS only)
                    refresh_cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
                    refresh_cookie.set_path("/");
                    refresh_cookie.set_max_age(tower_cookies::cookie::time::Duration::days(365));
                    cookies.add(refresh_cookie);

                    let response = LoginResponse {
                        message: "Login successful".to_string(),
                        token,
                        refresh_token: Some(refresh_token),
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

#[derive(Debug, serde::Deserialize)]
struct LogoutRequest {
    refresh_token: Option<String>,
}

// POST /api/logout
async fn logout(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    payload: Option<Json<LogoutRequest>>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Get refresh token from cookie or request body
    let refresh_token = cookies
        .get("refresh_token")
        .map(|c| c.value().to_string())
        .or_else(|| payload.and_then(|Json(p)| p.refresh_token.clone()));

    if let Some(token) = refresh_token {
        // Mark session as revoked in database using SHA-256 hash
        let update_result = sqlx::query(
            "UPDATE sessions SET is_revoked = TRUE WHERE refresh_token = SHA2(?, 256)"
        )
        .bind(token)
        .execute(&state.db.pool)
        .await;

        if let Err(e) = update_result {
            tracing::error!("Failed to revoke session on logout: {}", e);
        }
    }

    // Clear access token cookie (auth_token)
    let mut auth_cookie = Cookie::new("auth_token", "");
    auth_cookie.set_http_only(true);
    auth_cookie.set_path("/");
    auth_cookie.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
    cookies.add(auth_cookie);

    // Clear refresh token cookie (refresh_token)
    let mut refresh_cookie = Cookie::new("refresh_token", "");
    refresh_cookie.set_http_only(true);
    refresh_cookie.set_path("/");
    refresh_cookie.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
    cookies.add(refresh_cookie);

    (
        StatusCode::OK,
        Json(ApiResponse::success("Logged out successfully".to_string())),
    )
}

#[derive(sqlx::FromRow)]
struct SessionUser {
    id: String,
    user_id: i32,
    username: String,
    role: String,
}

// POST /api/refresh
async fn refresh(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
) -> (StatusCode, Json<ApiResponse<LoginResponse>>) {
    // 1. Get refresh token from cookie
    let refresh_token = match cookies.get("refresh_token").map(|c| c.value().to_string()) {
        Some(t) => t,
        None => return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Missing refresh token")),
        ),
    };

    // 2. Query sessions in DB (using SHA2-256)
    let session: Result<Option<SessionUser>, _> = sqlx::query_as(
        "SELECT s.id, s.user_id, u.username, u.role FROM sessions s \
         JOIN users u ON s.user_id = u.id \
         WHERE s.refresh_token = SHA2(?, 256) AND s.is_revoked = FALSE AND s.expires_at > NOW()"
    )
    .bind(&refresh_token)
    .fetch_optional(&state.db.pool)
    .await;

    let session_data = match session {
        Ok(Some(row)) => row,
        Ok(None) => return (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse::error("Invalid or expired session")),
        ),
        Err(e) => {
            tracing::error!("Database error in refresh token validation: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Internal database error")),
            );
        }
    };

    // 3. Generate new Access Token (JWT, 15 min)
    let new_access_token = match create_token(
        session_data.user_id,
        &session_data.username,
        &session_data.role,
        &state.config.jwt_secret,
    ) {
        Ok(t) => t,
        Err(_) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to generate access token")),
        ),
    };

    // 4. Generate new Refresh Token
    let new_refresh_token = generate_refresh_token();

    // 5. Update database: replace refresh_token, last_active = NOW()
    let update_result = sqlx::query(
        "UPDATE sessions SET refresh_token = SHA2(?, 256), last_active = NOW() WHERE id = ?"
    )
    .bind(&new_refresh_token)
    .bind(&session_data.id)
    .execute(&state.db.pool)
    .await;

    if let Err(e) = update_result {
        tracing::error!("Failed to update refresh token in database: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update session")),
        );
    }

    // 6. Query user details for LoginResponse
    let user_res: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
    )
    .bind(session_data.user_id)
    .fetch_one(&state.db.pool)
    .await;

    let user = match user_res {
        Ok(u) => u,
        Err(_) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("User not found")),
        ),
    };

    // 7. Set cookies
    let mut auth_cookie = Cookie::new("auth_token", new_access_token.clone());
    auth_cookie.set_http_only(true);
    auth_cookie.set_secure(false); // Set to true in production (HTTPS only)
    auth_cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
    auth_cookie.set_path("/");
    auth_cookie.set_max_age(tower_cookies::cookie::time::Duration::minutes(15));
    cookies.add(auth_cookie);

    let mut refresh_cookie = Cookie::new("refresh_token", new_refresh_token.clone());
    refresh_cookie.set_http_only(true);
    refresh_cookie.set_secure(false); // Set to true in production (HTTPS only)
    refresh_cookie.set_same_site(tower_cookies::cookie::SameSite::Lax);
    refresh_cookie.set_path("/");
    refresh_cookie.set_max_age(tower_cookies::cookie::time::Duration::days(365));
    cookies.add(refresh_cookie);

    let response = LoginResponse {
        message: "Token refreshed successfully".to_string(),
        token: new_access_token,
        refresh_token: Some(new_refresh_token),
        user: user.into(),
    };

    (StatusCode::OK, Json(ApiResponse::success(response)))
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

// GET /api/users - List users (superuser only) with pagination
async fn list_users(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<PaginationQuery>,
) -> (StatusCode, Json<ApiResponse<PaginatedUsersResponse>>) {
    if !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("Only superuser can list all users")),
        );
    }

    // Parse and validate pagination params
    let page = pagination.page.unwrap_or(1).max(1);
    let limit = pagination.limit.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * limit;

    // Get total count
    let total_result: Result<(i64,), _> = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db.pool)
        .await;

    let total = match total_result {
        Ok((count,)) => count as u32,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to count users")),
            );
        }
    };

    // Get paginated users
    let users: Result<Vec<User>, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users ORDER BY id ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db.pool)
    .await;

    match users {
        Ok(users) => {
            let user_responses: Vec<UserResponse> = users.into_iter().map(|u| u.into()).collect();
            let response = PaginatedUsersResponse {
                users: user_responses,
                page,
                limit,
                total,
            };
            (StatusCode::OK, Json(ApiResponse::success(response)))
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

// PATCH /api/users/:id - Partial profile update (no password)
async fn update_user(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateUserRequest>,
) -> (StatusCode, Json<ApiResponse<UserResponse>>) {
    // Users can only update their own profile unless they are superuser
    if auth_user.id != id && !auth_user.is_superuser() {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::error("You can only update your own profile")),
        );
    }

    // Check if at least one field is provided
    if payload.name.is_none() && payload.username.is_none() && payload.email.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("No fields to update")),
        );
    }

    // Fetch current user data
    let current_user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await;

    let current_user = match current_user {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("User not found")),
            );
        }
    };

    // Apply updates with validation
    let new_name = if let Some(ref name) = payload.name {
        if let Err(e) = validate_name(name) {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
        }
        name.clone()
    } else {
        current_user.name
    };

    let new_username = if let Some(ref username) = payload.username {
        if let Err(e) = validate_username(username) {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
        }
        username.clone()
    } else {
        current_user.username
    };

    let new_email = if let Some(ref email) = payload.email {
        if let Err(e) = validate_email(email) {
            return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
        }
        email.clone()
    } else {
        current_user.email
    };

    // Update database (no password, no role)
    let result = sqlx::query(
        "UPDATE users SET name = ?, username = ?, email = ? WHERE id = ?",
    )
    .bind(&new_name)
    .bind(&new_username)
    .bind(&new_email)
    .bind(id)
    .execute(&state.db.pool)
    .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let user_response = UserResponse {
                id,
                name: new_name,
                username: new_username,
                email: new_email,
                role: current_user.role,
            };
            (StatusCode::OK, Json(ApiResponse::success(user_response)))
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error("User not found")),
        ),
        Err(e) => {
            let error_msg = if e.to_string().contains("Duplicate entry") {
                "Username or email already exists"
            } else {
                "Failed to update user"
            };
            (StatusCode::BAD_REQUEST, Json(ApiResponse::error(error_msg)))
        }
    }
}

// PATCH /api/users/me/password - Change password with verification
async fn change_password(
    Extension(auth_user): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChangePasswordRequest>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    // Validate new password
    if let Err(e) = validate_password(&payload.new_password) {
        return (StatusCode::BAD_REQUEST, Json(ApiResponse::error(e)));
    }

    // Fetch current user's password hash
    let user: Result<User, _> = sqlx::query_as(
        "SELECT id, name, username, email, password_hash, role FROM users WHERE id = ?",
    )
    .bind(auth_user.id)
    .fetch_one(&state.db.pool)
    .await;

    let user = match user {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("User not found")),
            );
        }
    };

    // Verify current password
    match verify_password(&payload.current_password, &user.password_hash) {
        Ok(true) => {
            // Current password is correct, proceed with update
        }
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiResponse::error("Current password is incorrect")),
            );
        }
    }

    // Hash new password
    let new_password_hash = match hash_password(&payload.new_password) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error("Failed to hash password")),
            );
        }
    };

    // Update password
    let result = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&new_password_hash)
        .bind(auth_user.id)
        .execute(&state.db.pool)
        .await;

    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::success("Password updated successfully".to_string())),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error("Failed to update password")),
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
