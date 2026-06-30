mod config;
mod db;
mod models;
mod handlers;
mod auth;
mod middleware;

use axum::{middleware as axum_middleware, routing::get, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Database;
use crate::handlers::{health, users, gallery, video, audio, blog, notes, clipboard};
use crate::middleware::auth_middleware;

pub struct AppState {
    pub db: Database,
    pub config: Config,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "gitmeriz_api=debug,tower_http=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::from_env().expect("Failed to load configuration");

    // Initialize database
    let db = Database::new(&config.database_url)
        .await
        .expect("Failed to connect to database");

    // Run migrations
    db.migrate().await.expect("Failed to run migrations");

    let state = Arc::new(AppState { db, config: config.clone() });

    // Protected routes - require authentication
    let protected_routes = Router::new()
        .merge(users::protected_routes())
        .merge(gallery::router())
        .merge(video::router())
        .merge(audio::router())
        .merge(blog::router())
        .merge(notes::router())
        .merge(clipboard::router())
        .layer(axum_middleware::from_fn_with_state(state.clone(), auth_middleware));

    // Build router
    let app = Router::new()
        .route("/health", get(health::health_check))
        .nest("/api", Router::new()
            .merge(users::public_routes())
            .merge(protected_routes))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any))
        .with_state(state);

    // Start server
    let addr = format!("{}:{}", config.server_host, config.server_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    tracing::info!("Server running on http://{}", addr);

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
