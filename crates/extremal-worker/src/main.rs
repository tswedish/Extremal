use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use extremal_identity::Identity;
use extremal_strategies::default_strategies;
use extremal_worker_core::api::run_api_server;
use extremal_worker_core::client::ServerClient;
use extremal_worker_core::engine::{EngineCommand, EngineConfig, EngineSnapshot, run_engine};
use tokio::sync::{mpsc, watch};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "extremal-worker", about = "Extremal search worker")]
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

    /// Dashboard relay server URL (e.g. ws://localhost:4000/ws/worker).
    /// Streams real-time search telemetry to the dashboard.
    #[arg(long)]
    dashboard: Option<String>,

    /// Port for the worker control API (0 = auto-assign).
    #[arg(long, default_value = "0")]
    api_port: u16,

    /// Max steps for score-aware polish walk per valid graph (0 = disable).
    #[arg(long, default_value = "100")]
    polish_max_steps: u64,

    /// Tabu tenure during polish walk.
    #[arg(long, default_value = "25")]
    polish_tabu_tenure: u64,

    /// Score-bias threshold: prefer balanced kc/ei in beam when violations <= this.
    #[arg(long, default_value = "3")]
    score_bias_threshold: u64,

    /// ILS restarts: perturbation+re-polish cycles per valid graph (0 = disabled).
    #[arg(long, default_value = "0")]
    polish_ils_restarts: u64,

    /// ILS perturbation edges: random valid-preserving flips between polish walks.
    #[arg(long, default_value = "3")]
    polish_ils_perturb: u64,

    /// Enable 2-opt paired edge flips in polish (escapes single-flip basins).
    #[arg(long, default_value = "false", action = clap::ArgAction::Set)]
    polish_2opt: bool,

    /// SA initial temperature (higher = more exploration through invalid states).
    #[arg(long, default_value = "10.0")]
    sa_initial_temp: f64,

    /// SA violation weight in objective (lower = more willing to traverse invalid states).
    #[arg(long, default_value = "10")]
    sa_violation_weight: i64,

    /// Construct: max tabu iterations to repair near-valid constructions.
    #[arg(long, default_value = "10000")]
    repair_max_iters: u64,

    /// Construct: max violations to attempt repair (0 = only exact valid).
    #[arg(long, default_value = "10")]
    repair_threshold: u64,
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
    let identity: Option<Arc<Identity>> = if let Some(ref path) = cli.signing_key {
        match Identity::load(std::path::Path::new(path)) {
            Ok(id) => {
                tracing::info!(key_id = %id.key_id, "loaded signing key");
                Some(Arc::new(id))
            }
            Err(e) => {
                tracing::error!("failed to load signing key: {e}");
                return;
            }
        }
    } else {
        let default_path = std::path::Path::new(".config/extremal/key.json");
        if default_path.exists() {
            match Identity::load(default_path) {
                Ok(id) => {
                    tracing::info!(key_id = %id.key_id, "loaded default signing key");
                    Some(Arc::new(id))
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
        tracing::error!("no signing key found — run `extremal-cli keygen` first, or use --offline");
        return;
    }

    // Build strategy config
    let strategy_config = serde_json::json!({
        "beam_width": cli.beam_width,
        "max_depth": cli.max_depth,
        "focused": cli.focused,
        "target_k": cli.target_k,
        "target_ell": cli.target_ell,
        "polish_max_steps": cli.polish_max_steps,
        "polish_tabu_tenure": cli.polish_tabu_tenure,
        "score_bias_threshold": cli.score_bias_threshold,
        "polish_ils_restarts": cli.polish_ils_restarts,
        "polish_ils_perturb": cli.polish_ils_perturb,
        "polish_2opt": cli.polish_2opt,
        "sa_initial_temp": cli.sa_initial_temp,
        "sa_violation_weight": cli.sa_violation_weight,
        "repair_max_iters": cli.repair_max_iters,
        "repair_threshold": cli.repair_threshold,
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
        dashboard_url: cli.dashboard,
    };

    // Build server client
    let client = if !cli.offline {
        Some(ServerClient::new(&cli.server, identity.clone()))
    } else {
        None
    };

    // Register strategies
    let strategies: Vec<Arc<dyn extremal_worker_api::SearchStrategy>> =
        default_strategies().into_iter().map(Arc::from).collect();

    // Shutdown channel (Ctrl+C)
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Ctrl+C received, shutting down...");
        let _ = shutdown_tx.send(true);
    });

    // Create command channel for API → engine communication
    let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>(32);

    // Create initial snapshot for the watch channel
    let initial_snapshot = EngineSnapshot {
        state: extremal_worker_api::WorkerState::Idle,
        round: 0,
        n: config.n,
        strategy: config.strategy_id.clone(),
        worker_id: config
            .metadata
            .as_ref()
            .and_then(|m| m.get("worker_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string(),
        key_id: identity
            .as_ref()
            .map(|id| id.key_id.as_str().to_string())
            .unwrap_or_default(),
        config: extremal_worker_core::engine::ConfigSnapshot { params: vec![] },
        metrics: extremal_worker_core::engine::EngineMetrics {
            total_discoveries: 0,
            total_submitted: 0,
            total_admitted: 0,
            submit_buffer_size: 0,
            known_cids_count: 0,
            server_cids_count: 0,
            last_round_ms: 0,
        },
    };
    let (snapshot_tx, snapshot_rx) = watch::channel(initial_snapshot);

    // Start worker HTTP API server
    let api_addr_str = {
        let bind_addr: SocketAddr = ([0, 0, 0, 0], cli.api_port).into();
        match run_api_server(bind_addr, cmd_tx, snapshot_rx).await {
            Ok(addr) => {
                let api_url = format!("http://0.0.0.0:{}", addr.port());
                tracing::info!(api = %api_url, "worker API server ready");
                Some(api_url)
            }
            Err(e) => {
                tracing::warn!("failed to start worker API server: {e}");
                None
            }
        }
    };

    run_engine(
        config,
        strategies,
        client,
        identity,
        shutdown_rx,
        Some(cmd_rx),
        Some(snapshot_tx),
        api_addr_str,
    )
    .await;
}
