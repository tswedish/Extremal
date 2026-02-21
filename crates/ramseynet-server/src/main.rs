use axum::{routing::get, Json, Router};
use clap::Parser;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

#[derive(Parser, Debug)]
#[command(name = "ramseynet-server", about = "RamseyNet protocol server")]
struct Config {
    /// Port to listen on
    #[arg(long, default_value = "3001")]
    port: u16,

    /// Path to SQLite database
    #[arg(long, default_value = "ramseynet.db")]
    db_path: String,
}

async fn health() -> Json<Value> {
    Json(json!({
        "name": "RamseyNet",
        "version": ramseynet_types::PROTOCOL_VERSION,
        "status": "ok"
    }))
}

async fn list_challenges() -> Json<Value> {
    Json(json!({ "challenges": [] }))
}

async fn list_records() -> Json<Value> {
    Json(json!({ "records": [] }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ramseynet=info".into()),
        )
        .init();

    let config = Config::parse();

    let app = Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .route("/api/challenges", get(list_challenges))
        .route("/api/records", get(list_records))
        .layer(CorsLayer::permissive());

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("RamseyNet server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
