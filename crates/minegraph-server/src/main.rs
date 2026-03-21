use clap::Parser;
use minegraph_identity::Identity;
use minegraph_store::Store;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "minegraph-server", about = "MineGraph leaderboard API server")]
struct Config {
    /// Port to listen on.
    #[arg(long, env = "PORT", default_value = "3001")]
    port: u16,

    /// PostgreSQL connection URL.
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://localhost/minegraph"
    )]
    database_url: String,

    /// Maximum leaderboard entries per n.
    #[arg(long, env = "LEADERBOARD_CAPACITY", default_value = "500")]
    leaderboard_capacity: i32,

    /// Maximum k for histogram scoring.
    #[arg(long, env = "MAX_K", default_value = "5")]
    max_k: u32,

    /// Run database migrations on startup.
    #[arg(long)]
    migrate: bool,

    /// Path to server signing key.
    #[arg(long, env = "SERVER_KEY_PATH")]
    server_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::parse();

    // Connect to PostgreSQL
    tracing::info!("connecting to database...");
    let pool = sqlx::PgPool::connect(&config.database_url).await?;
    let store = Store::new(pool);

    // Run migrations if requested
    if config.migrate {
        tracing::info!("running database migrations...");
        store.migrate().await?;
        tracing::info!("migrations complete");
    }

    // Trim any over-capacity leaderboards from prior runs
    let summaries = store.list_leaderboard_ns().await.unwrap_or_default();
    for s in &summaries {
        if s.entry_count > config.leaderboard_capacity as i64 {
            let trimmed = store
                .trim_leaderboard(s.n, config.leaderboard_capacity)
                .await
                .unwrap_or(0);
            if trimmed > 0 {
                tracing::info!(n = s.n, trimmed, "trimmed over-capacity leaderboard");
            }
        }
    }

    // Load or generate server identity
    let server_identity = if let Some(path) = &config.server_key {
        tracing::info!("loading server key from {path}");
        Identity::load(std::path::Path::new(path))?
    } else {
        tracing::warn!("no --server-key provided, generating ephemeral server identity");
        let id = Identity::generate(Some("minegraph-server".into()));
        tracing::info!("server key_id: {}", id.key_id);
        id
    };

    // Build application state
    let (events_tx, _) = broadcast::channel(256);
    let state = minegraph_server::state::AppState {
        store,
        server_identity: Arc::new(server_identity),
        leaderboard_capacity: config.leaderboard_capacity,
        max_k: config.max_k,
        events_tx,
    };

    // Background task: snapshot leaderboard stats every 10 minutes
    let snapshot_store = state.store.clone();
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(600); // 10 minutes
        loop {
            tokio::time::sleep(interval).await;
            // Snapshot all active leaderboards
            match snapshot_store.list_leaderboard_ns().await {
                Ok(boards) => {
                    for board in &boards {
                        if let Err(e) = snapshot_store.capture_snapshot(board.n).await {
                            tracing::warn!(n = board.n, "snapshot failed: {e}");
                        }
                    }
                    if !boards.is_empty() {
                        tracing::debug!(count = boards.len(), "leaderboard snapshots captured");
                    }
                }
                Err(e) => tracing::warn!("snapshot task failed: {e}"),
            }
        }
    });

    // Build router
    let app = minegraph_server::create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("MineGraph server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
