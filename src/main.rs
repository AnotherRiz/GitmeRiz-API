mod config;
mod db;
mod models;
mod handlers;
mod auth;
mod middleware;
mod media;
mod error_page;
mod validation;

use axum::{middleware as axum_middleware, routing::get, Router};
use axum::http::{header, HeaderValue, Method};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tower_cookies::CookieManagerLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Database;
use crate::handlers::{health, users, gallery, video, audio, blog, notes, clipboard};
use crate::middleware::auth_middleware;

pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub image_semaphore: Arc<Semaphore>,
    pub video_semaphore: Arc<Semaphore>,
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

    // Initialize semaphore for parallel image processing (memory ceiling)
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let image_permit_count = cpu_count.clamp(4, 8);
    tracing::info!("Image processing semaphore initialized with {} permits", image_permit_count);

    // Initialize semaphore for FFmpeg video processing (CPU-heavy, limit to 1-2)
    let video_permit_count = (cpu_count / 2).clamp(1, 2);
    tracing::info!("Video processing semaphore initialized with {} permits", video_permit_count);

    let state = Arc::new(AppState { 
        db, 
        config: config.clone(),
        image_semaphore: Arc::new(Semaphore::new(image_permit_count)),
        video_semaphore: Arc::new(Semaphore::new(video_permit_count)),
    });

    // Resume any unfinished video processing tasks
    let state_clone = state.clone();
    tokio::spawn(async move {
        handlers::video::resume_processing_on_startup(state_clone).await;
    });

    // Protected routes - require authentication
    let protected_routes = Router::new()
        .merge(users::protected_routes())
        .merge(gallery::protected_routes())
        .merge(video::protected_routes())
        .merge(audio::router())
        .merge(blog::router())
        .merge(notes::router())
        .merge(clipboard::router())
        .layer(axum_middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(axum::extract::DefaultBodyLimit::disable());

    // CORS configuration - must be specific origin when using credentials
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:5173".parse::<HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::PUT, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE])
        .allow_credentials(true)
        .expose_headers([header::SET_COOKIE]);

    // Build router
    let app = Router::new()
        .route("/health", get(health::health_check))
        .merge(users::public_routes())
        .merge(gallery::public_routes())
        .merge(video::public_routes())
        .merge(protected_routes)
        .layer(cors)
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

use std::net::SocketAddr;

    // Start server
    let addr = format!("{}:{}", config.server_host, config.server_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    tracing::info!("Server running on http://{}", addr);

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("Server error");
}
