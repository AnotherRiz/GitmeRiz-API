use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// Role enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Superuser,
    Admin,
    User,
}

impl Role {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Superuser => "superuser",
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "superuser" => Some(Role::Superuser),
            "admin" => Some(Role::Admin),
            "user" => Some(Role::User),
            _ => None,
        }
    }

    // Permission checks
    pub fn can_view_all_media(&self) -> bool {
        matches!(self, Role::Superuser)
    }

    pub fn can_write_blog(&self) -> bool {
        matches!(self, Role::Superuser | Role::Admin)
    }
}

// Database model
#[derive(Debug, FromRow)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
}

// Response model (without password)
#[derive(Debug, Clone, Serialize)]
pub struct UserResponse {
    pub id: i32,
    pub name: String,
    pub username: String,
    pub email: String,
    pub role: String,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id,
            name: user.name,
            username: user.username,
            email: user.email,
            role: user.role,
        }
    }
}

// Request models
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    // NOTE: no password, no role here - password updated via separate endpoint
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedUsersResponse {
    pub users: Vec<UserResponse>,
    pub page: u32,
    pub limit: u32,
    pub total: u32,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub message: String,
    pub token: String,
    pub refresh_token: Option<String>,
    pub user: UserResponse,
}

// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i32,        // user id
    pub username: String,
    pub role: String,
    pub exp: i64,        // expiration time
    pub iat: i64,        // issued at
}

// Authenticated user (extracted from JWT)
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i32,
    #[allow(dead_code)]
    pub username: String,
    pub role: Role,
}

impl AuthUser {
    pub fn can_view_all_media(&self) -> bool {
        self.role.can_view_all_media()
    }

    pub fn can_write_blog(&self) -> bool {
        self.role.can_write_blog()
    }

    pub fn is_superuser(&self) -> bool {
        matches!(self.role, Role::Superuser)
    }
}

// Generic API response
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}
