//! MineGraph leaderboard API server.
//!
//! Pure REST API — no static file serving. Web UIs are separate apps.

pub mod error;
pub mod handlers;
pub mod state;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use state::AppState;

/// Build the Axum router with all API routes.
pub fn create_router(state: AppState) -> Router {
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
        // Submissions
        .route(
            "/submit",
            axum::routing::post(handlers::submit::submit_graph),
        )
        .route(
            "/verify",
            axum::routing::post(handlers::submit::verify_graph),
        )
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
        // Workers
        .route(
            "/workers",
            axum::routing::get(handlers::workers::list_workers),
        )
        .route(
            "/workers/heartbeat",
            axum::routing::post(handlers::workers::worker_heartbeat),
        )
        // SSE events stream
        .route(
            "/events",
            axum::routing::get(handlers::events::event_stream),
        );

    Router::new()
        .nest("/api", api)
        .route("/", axum::routing::get(handlers::health::health))
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1 MB
        .layer(CorsLayer::permissive()) // tighten in production via env var
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
