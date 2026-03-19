use std::sync::Arc;

use clap::Parser;
use minegraph_identity::Identity;
use minegraph_strategies::default_strategies;
use minegraph_worker_core::client::ServerClient;
use minegraph_worker_core::engine::{EngineConfig, run_engine};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "minegraph-worker", about = "MineGraph search worker")]
struct Cli {
    /// Server URL.
    #[arg(long, default_value = "http://localhost:3001")]
    server: String,

    /// Target vertex count.
    #[arg(long)]
    n: u32,

    /// Search strategy.
    #[arg(long, default_value = "tree2")]
    strategy: String,

    /// Ramsey parameter k (clique size in graph).
    #[arg(long, default_value = "5")]
    target_k: u32,

    /// Ramsey parameter ell (clique size in complement).
    #[arg(long, default_value = "5")]
    target_ell: u32,

    /// Maximum iterations per round.
    #[arg(long, default_value = "100000")]
    max_iters: u64,

    /// Beam width for tree2.
    #[arg(long, default_value = "100")]
    beam_width: u64,

    /// Max search depth for tree2.
    #[arg(long, default_value = "10")]
    max_depth: u64,

    /// Sample bias for leaderboard seeding (0.0-1.0).
    #[arg(long, default_value = "0.8")]
    sample_bias: f64,

    /// Leaderboard graphs to fetch for seeding.
    #[arg(long, default_value = "50")]
    leaderboard_sample_size: u32,

    /// Max known CIDs to track for dedup.
    #[arg(long, default_value = "50000")]
    max_known_cids: usize,

    /// Only flip edges participating in violations.
    #[arg(long, default_value = "false", action = clap::ArgAction::Set)]
    focused: bool,

    /// Noise flips to apply to seed graphs.
    #[arg(long, default_value = "0")]
    noise_flips: u32,

    /// Max submissions per round (0 = unlimited).
    #[arg(long, default_value = "20")]
    max_submissions_per_round: usize,

    /// Run without server (local search only).
    #[arg(long)]
    offline: bool,

    /// Path to signing key file.
    #[arg(long)]
    signing_key: Option<String>,

    /// Metadata JSON string (e.g. '{"worker_id":"w1","commit_hash":"abc123"}').
    /// Attached to submissions and shown in the dashboard. Max 4KB.
    #[arg(long)]
    metadata: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load signing identity
    let identity = if let Some(ref path) = cli.signing_key {
        match Identity::load(std::path::Path::new(path)) {
            Ok(id) => {
                tracing::info!(key_id = %id.key_id, "loaded signing key");
                Some(id)
            }
            Err(e) => {
                tracing::error!("failed to load signing key: {e}");
                return;
            }
        }
    } else {
        let default_path = std::path::Path::new(".config/minegraph/key.json");
        if default_path.exists() {
            match Identity::load(default_path) {
                Ok(id) => {
                    tracing::info!(key_id = %id.key_id, "loaded default signing key");
                    Some(id)
                }
                Err(e) => {
                    tracing::warn!("failed to load default key: {e}");
                    None
                }
            }
        } else {
            None
        }
    };

    if identity.is_none() && !cli.offline {
        tracing::error!(
            "no signing key found — run `minegraph-cli keygen` first, or use --offline"
        );
        return;
    }

    // Build strategy config
    let strategy_config = serde_json::json!({
        "beam_width": cli.beam_width,
        "max_depth": cli.max_depth,
        "focused": cli.focused,
        "target_k": cli.target_k,
        "target_ell": cli.target_ell,
    });

    // Parse metadata JSON
    let metadata: Option<serde_json::Value> = cli.metadata.as_deref().and_then(|s| {
        if s.len() > 4096 {
            tracing::error!("metadata exceeds 4KB limit, ignoring");
            return None;
        }
        match serde_json::from_str(s) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::error!("invalid metadata JSON: {e}");
                None
            }
        }
    });

    let config = EngineConfig {
        n: cli.n,
        max_iters: cli.max_iters,
        server_url: cli.server.clone(),
        strategy_id: cli.strategy.clone(),
        strategy_config,
        sample_bias: cli.sample_bias,
        leaderboard_sample_size: cli.leaderboard_sample_size,
        max_known_cids: cli.max_known_cids,
        offline: cli.offline,
        noise_flips: cli.noise_flips,
        max_submissions_per_round: cli.max_submissions_per_round,
        metadata,
    };

    // Build server client
    let client = if !cli.offline {
        Some(ServerClient::new(&cli.server, identity))
    } else {
        None
    };

    // Register strategies
    let strategies: Vec<Arc<dyn minegraph_worker_api::SearchStrategy>> =
        default_strategies().into_iter().map(Arc::from).collect();

    // Shutdown channel (Ctrl+C)
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Ctrl+C received, shutting down...");
        let _ = shutdown_tx.send(true);
    });

    run_engine(config, strategies, client, shutdown_rx).await;
}
