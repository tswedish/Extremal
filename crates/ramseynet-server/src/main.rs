use std::sync::Arc;

use clap::Parser;
use ramseynet_ledger::Ledger;
use ramseynet_server::AppState;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ramseynet=info".into()),
        )
        .init();

    let config = Config::parse();

    let ledger = Arc::new(Ledger::open(&config.db_path)?);

    let state = Arc::new(AppState { ledger });
    let app = ramseynet_server::create_router(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("RamseyNet server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
