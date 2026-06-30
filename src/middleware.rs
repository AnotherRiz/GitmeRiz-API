use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use crate::auth::validate_token;
use crate::models::ApiResponse;
use crate::AppState;

// Extract token from Authorization header
fn extract_token(req: &Request) -> Option<String> {
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
}

// Authentication middleware - validates JWT and injects AuthUser
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = match extract_token(&req) {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ApiResponse::<()>::error("Missing or invalid authorization header")),
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
