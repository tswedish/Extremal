use clap::Parser;
use extremal_identity::Identity;
use extremal_store::Store;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "extremal-server", about = "Extremal leaderboard API server")]
struct Config {
    /// Port to listen on.
    #[arg(long, env = "PORT", default_value = "3001")]
    port: u16,

    /// PostgreSQL connection URL.
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://localhost/extremal"
    )]
    database_url: String,

    /// Database password (injected into DATABASE_URL as &password=<value>).
    /// Use this to keep the password out of the DATABASE_URL env var.
    #[arg(long, env = "DB_PASSWORD")]
    db_password: Option<String>,

    /// Maximum leaderboard entries per n.
    #[arg(long, env = "LEADERBOARD_CAPACITY", default_value = "500")]
    leaderboard_capacity: i32,

    /// Maximum k for histogram scoring.
    #[arg(long, env = "MAX_K", default_value = "5")]
    max_k: u32,

    /// Maximum allowed graph vertex count (graph6 supports up to 62).
    #[arg(long, env = "MAX_N", default_value = "62")]
    max_n: u32,

    /// Maximum database connections in the pool.
    #[arg(long, env = "DB_MAX_CONNECTIONS", default_value = "10")]
    db_max_connections: u32,

    /// Run database migrations on startup.
    #[arg(long)]
    migrate: bool,

    /// Path to server signing key.
    #[arg(long, env = "SERVER_KEY_PATH")]
    server_key: Option<String>,

    /// Allowed CORS origins (comma-separated). If unset, allows all origins (dev mode).
    #[arg(long, env = "ALLOWED_ORIGINS")]
    allowed_origins: Option<String>,
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

    // Connect to PostgreSQL with pool configuration
    tracing::info!("connecting to database...");
    let mut db_url = config.database_url.clone();
    if let Some(ref password) = config.db_password {
        let sep = if db_url.contains('?') { "&" } else { "?" };
        db_url.push_str(&format!("{sep}password={password}"));
    }
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&db_url)
        .await?;
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
        let id = Identity::generate(Some("extremal-server".into()));
        tracing::info!("server key_id: {}", id.key_id);
        id
    };

    // Parse allowed origins
    let allowed_origins = config.allowed_origins.map(|s| {
        s.split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    });

    // Build application state
    let (events_tx, _) = broadcast::channel(256);
    let state = extremal_server::state::AppState {
        store,
        server_identity: Arc::new(server_identity),
        leaderboard_capacity: config.leaderboard_capacity,
        max_k: config.max_k,
        max_n: config.max_n,
        events_tx,
        allowed_origins,
    };

    // Background task: snapshot leaderboard stats every 10 minutes
    // Uses advisory lock to avoid duplicate work across multiple instances.
    let snapshot_store = state.store.clone();
    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let snapshot_token = shutdown_token.clone();
    tokio::spawn(async move {
        let interval = Duration::from_secs(600); // 10 minutes
        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = snapshot_token.cancelled() => break,
            }
            // Try to acquire advisory lock — only one instance runs snapshots
            if !snapshot_store.try_advisory_lock(1).await.unwrap_or(false) {
                tracing::debug!("snapshot: another instance holds the lock, skipping");
                continue;
            }
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
            let _ = snapshot_store.advisory_unlock(1).await;
        }
    });

    // Build router
    let app = extremal_server::create_router(state);

    // Start server with graceful shutdown
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Extremal server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal(shutdown_token))
    .await?;

    Ok(())
}

async fn shutdown_signal(cancel: tokio_util::sync::CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down"),
    }

    #[cfg(not(unix))]
    ctrl_c.await.ok();

    cancel.cancel();
}
