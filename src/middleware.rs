use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use tower_cookies::Cookies;

use crate::auth::validate_token;
use crate::models::ApiResponse;
use crate::AppState;

// Extract token from cookie or Authorization header
// Priority: Cookie first (preferred), then Authorization header (backward compatibility)
fn extract_token(cookies: &Cookies, req: &Request) -> Option<String> {
    // Priority 1: Read from cookie (preferred)
    cookies
        .get("auth_token")
        .map(|c| c.value().to_string())
        // Priority 2: Fallback to Authorization header (backward compatibility)
        .or_else(|| {
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    if value.starts_with("Bearer ") {
                        Some(value[7..].to_string())
                    } else {
                        None
                    }
                })
        })
}

// Authentication middleware - validates JWT and injects AuthUser
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    cookies: Cookies,
    mut req: Request,
    next: Next,
) -> Response {
    let token = match extract_token(&cookies, &req) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiResponse::<()>::error("Missing or invalid authorization")),
            )
                .into_response();
        }
    };

    match validate_token(&token, &state.config.jwt_secret) {
        Ok(auth_user) => {
            req.extensions_mut().insert(auth_user);
            next.run(req).await
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse::<()>::error("Invalid or expired token")),
        )
            .into_response(),
    }
}
