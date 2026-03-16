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

    /// Maximum entries per (k, ell, n) leaderboard. On startup, any
    /// leaderboard exceeding this capacity is trimmed to fit.
    #[arg(long, default_value = "500")]
    leaderboard_capacity: u32,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    // Build log filter based on verbosity level (overridden by RUST_LOG env var)
    let default_filter = match config.verbose {
        0 => "ramseynet=info",
        1 => "ramseynet=debug,tower_http=info",
        2 => "ramseynet=debug,tower_http=debug",
        _ => "ramseynet=trace,tower_http=trace,axum=trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .init();

    let ledger = Arc::new(Ledger::open_with_capacity(
        &config.db_path,
        config.leaderboard_capacity,
    )?);

    let state = Arc::new(AppState { ledger });
    let app = ramseynet_server::create_router(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!(
        port = config.port,
        db = %config.db_path,
        leaderboard_capacity = config.leaderboard_capacity,
        verbosity = config.verbose,
        "RamseyNet server listening on {addr}"
    );

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
