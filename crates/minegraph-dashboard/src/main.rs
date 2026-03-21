use std::collections::HashSet;

use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod protocol;
mod server;
mod state;

#[derive(Parser)]
#[command(
    name = "minegraph-dashboard",
    about = "MineGraph worker dashboard relay server"
)]
struct Config {
    /// Port to listen on.
    #[arg(long, env = "DASHBOARD_PORT", default_value = "4000")]
    port: u16,

    /// Path to JSON file of allowed key_ids. If omitted, all keys accepted.
    #[arg(long)]
    allow_keys: Option<String>,

    /// Maximum concurrent worker connections.
    #[arg(long, default_value = "50")]
    max_workers: usize,

    /// Directory to serve static files from (built dashboard UI).
    #[arg(long)]
    static_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::parse();

    // Load allow-list
    let allowed_keys = if let Some(ref path) = config.allow_keys {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read allow-keys file: {e}"))?;
        let keys: Vec<String> = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("invalid allow-keys JSON (expected string array): {e}"))?;
        let set: HashSet<String> = keys.into_iter().collect();
        tracing::info!(count = set.len(), "loaded allow-list");
        set
    } else {
        tracing::info!("no allow-list configured, accepting all workers");
        HashSet::new()
    };

    let dashboard_state = state::DashboardState::new(config.max_workers, allowed_keys);

    // Build router
    let mut app = Router::new()
        // WebSocket endpoints
        .route("/ws/worker", axum::routing::get(server::ws_worker))
        .route("/ws/ui", axum::routing::get(server::ws_ui))
        // REST API
        .route("/api/workers", axum::routing::get(server::list_workers))
        .route("/api/config", axum::routing::get(server::get_config))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(dashboard_state);

    // Optionally serve static files
    if let Some(ref dir) = config.static_dir {
        tracing::info!(dir, "serving static files");
        app = app.nest_service(
            "/",
            tower_http::services::ServeDir::new(dir).fallback(
                tower_http::services::ServeFile::new(format!("{}/index.html", dir)),
            ),
        );
    }

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("MineGraph dashboard listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
