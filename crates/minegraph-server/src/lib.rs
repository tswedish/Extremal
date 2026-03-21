//! MineGraph leaderboard API server.
//!
//! Pure REST API — no static file serving. Web UIs are separate apps.

pub mod error;
pub mod handlers;
pub mod state;

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use state::AppState;

/// Build the Axum router with all API routes.
pub fn create_router(state: AppState) -> Router {
    // Rate limiting: tight limit for CPU-intensive submit/verify endpoints
    let submit_governor = GovernorConfigBuilder::default()
        .per_second(5) // 5 requests/sec per IP
        .burst_size(10)
        .finish()
        .unwrap();

    // Routes that trigger CPU-intensive scoring get their own rate limit
    let scoring_routes = Router::new()
        .route(
            "/submit",
            axum::routing::post(handlers::submit::submit_graph),
        )
        .route(
            "/verify",
            axum::routing::post(handlers::submit::verify_graph),
        )
        .layer(GovernorLayer::new(submit_governor));

    // Global rate limit for all other API routes
    let global_governor = GovernorConfigBuilder::default()
        .per_second(100) // 100 requests/sec per IP
        .burst_size(200)
        .finish()
        .unwrap();

    let api = Router::new()
        // Health
        .route("/health", axum::routing::get(handlers::health::health))
        // Leaderboards
        .route(
            "/leaderboards",
            axum::routing::get(handlers::leaderboard::list_leaderboards),
        )
        .route(
            "/leaderboards/{n}",
            axum::routing::get(handlers::leaderboard::get_leaderboard),
        )
        .route(
            "/leaderboards/{n}/threshold",
            axum::routing::get(handlers::leaderboard::get_threshold),
        )
        .route(
            "/leaderboards/{n}/cids",
            axum::routing::get(handlers::leaderboard::get_cids),
        )
        .route(
            "/leaderboards/{n}/graphs",
            axum::routing::get(handlers::leaderboard::get_graphs),
        )
        .route(
            "/leaderboards/{n}/history",
            axum::routing::get(handlers::history::get_history),
        )
        .route(
            "/leaderboards/{n}/export",
            axum::routing::get(handlers::history::export_graph6),
        )
        .route(
            "/leaderboards/{n}/export/csv",
            axum::routing::get(handlers::history::export_csv),
        )
        // Submissions (with tighter rate limiting)
        .merge(scoring_routes)
        .route(
            "/submissions/{cid}",
            axum::routing::get(handlers::submit::get_submission),
        )
        // Identity
        .route(
            "/keys",
            axum::routing::post(handlers::identity::register_key),
        )
        .route(
            "/keys/{key_id}",
            axum::routing::get(handlers::identity::get_key),
        )
        .route(
            "/keys/{key_id}/submissions",
            axum::routing::get(handlers::identity::get_key_submissions),
        )
        // SSE events stream
        .route(
            "/events",
            axum::routing::get(handlers::events::event_stream),
        )
        .layer(GovernorLayer::new(global_governor));

    // CORS: use specific origins if configured, otherwise permissive (dev mode)
    let cors = if let Some(ref origins) = state.allowed_origins {
        let origins: Vec<_> = origins.iter().filter_map(|o| o.parse().ok()).collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([axum::http::header::CONTENT_TYPE])
    } else {
        CorsLayer::permissive()
    };

    Router::new()
        .nest("/api", api)
        .route("/", axum::routing::get(handlers::health::health))
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1 MB
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
