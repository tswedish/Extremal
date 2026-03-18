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

    // Build router
    let app = minegraph_server::create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("MineGraph server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
