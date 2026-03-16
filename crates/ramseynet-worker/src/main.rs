use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use tokio::sync::{mpsc, watch};
use tracing::info;

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_strategies::default_strategies;
use ramseynet_verifier::scoring::GraphScore;
use ramseynet_worker::viz::{SearchSnapshot, VizHandle};
use ramseynet_worker_api::{ProgressInfo, WorkerEvent};
use ramseynet_worker_core::engine::EngineConfig;
use ramseynet_worker_core::{run_engine, InitMode, VizBridge};

/// VizBridge implementation that forwards engine events to the VizHandle.
struct VizBridgeImpl {
    handle: Arc<VizHandle>,
    last_snapshot: Mutex<Instant>,
    ema: Mutex<(u64, Instant, f64)>,
}

impl VizBridgeImpl {
    fn new(handle: Arc<VizHandle>) -> Self {
        let now = Instant::now();
        Self {
            handle,
            last_snapshot: Mutex::new(now),
            ema: Mutex::new((0, now, 0.0)),
        }
    }
}

const EMA_ALPHA: f64 = 0.3;

impl VizBridge for VizBridgeImpl {
    fn on_progress(&self, graph: &AdjacencyMatrix, info: &ProgressInfo) {
        let now = Instant::now();
        {
            let mut last = self.last_snapshot.lock().unwrap();
            if now.duration_since(*last).as_millis() < 50 {
                return;
            }
            *last = now;
        }

        let elapsed_ms = self.handle.elapsed_ms();
        let throughput = {
            let mut ema = self.ema.lock().unwrap();
            let dt = now.duration_since(ema.1).as_secs_f64();
            let d_iters = info.iteration.saturating_sub(ema.0);
            let instant_rate = if dt > 0.0 { d_iters as f64 / dt } else { ema.2 };
            let smoothed = if info.iteration < ema.0 || ema.2 == 0.0 {
                instant_rate
            } else {
                EMA_ALPHA * instant_rate + (1.0 - EMA_ALPHA) * ema.2
            };
            *ema = (info.iteration, now, smoothed);
            smoothed
        };

        let snapshot = SearchSnapshot {
            graph: ramseynet_graph::rgxf::to_json(graph),
            n: info.n,
            k: info.k,
            ell: info.ell,
            strategy: info.strategy.clone(),
            iteration: info.iteration,
            max_iters: info.max_iters,
            valid: info.valid,
            edges: graph.num_edges() as u32,
            violation_score: info.violation_score,
            k_cliques: info.k_cliques,
            ell_indsets: info.ell_indsets,
            elapsed_ms,
            throughput,
        };
        self.handle.update_snapshot(snapshot);
    }

    fn on_discovery(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        score: GraphScore,
    ) {
        self.handle
            .submit_discovery(graph, n, strategy, iteration, false, score);
    }
}

#[derive(Parser)]
#[command(name = "ramseynet-worker", about = "RamseyNet search worker")]
struct Cli {
    /// Server URL
    #[arg(long, default_value = "http://localhost:3001")]
    server: String,

    /// Ramsey parameter k (omit to start in idle mode)
    #[arg(long)]
    k: Option<u32>,

    /// Ramsey parameter ell (omit to start in idle mode)
    #[arg(long)]
    ell: Option<u32>,

    /// Target vertex count n (omit to start in idle mode)
    #[arg(long)]
    n: Option<u32>,

    /// Search strategy
    #[arg(long, default_value = "tree2")]
    strategy: String,

    /// Maximum iterations per search attempt
    #[arg(long, default_value = "100000")]
    max_iters: u64,

    /// Port for worker web-app (viz + controls)
    #[arg(long)]
    port: Option<u16>,

    /// Disable backoff delay between failed rounds
    #[arg(long)]
    no_backoff: bool,

    /// Offline mode: no server interaction
    #[arg(long)]
    offline: bool,

    /// Graph initialization mode
    #[arg(long, default_value = "perturbed-paley")]
    init: String,

    /// Number of random edge flips for seed graph noise
    #[arg(long)]
    noise_flips: Option<u32>,

    /// Sampling bias: 0.0 = uniform, 1.0 = top-heavy
    #[arg(long, default_value = "0.5")]
    sample_bias: f64,

    /// Graphs to fetch from server for leaderboard seeding
    #[arg(long, default_value = "100")]
    leaderboard_sample_size: u32,

    /// Discovery buffer capacity
    #[arg(long, default_value = "1000")]
    collector_capacity: usize,

    /// Maximum known CIDs for deduplication
    #[arg(long, default_value = "10000")]
    max_known_cids: usize,

    /// Beam width for tree search
    #[arg(long, default_value = "100")]
    beam_width: usize,

    /// Maximum search depth for tree search
    #[arg(long, default_value = "10")]
    max_depth: u32,

    /// Path to MineGraph signing key (JSON file with key_id + secret_key).
    /// If not provided, checks .config/minegraph/key.json in the current directory.
    #[arg(long)]
    signing_key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    info!(
        server = %cli.server,
        k = ?cli.k,
        ell = ?cli.ell,
        n = ?cli.n,
        strategy = %cli.strategy,
        init = %cli.init,
        "starting ramseynet worker"
    );

    // Register strategies
    let strategies = default_strategies();
    let available: Vec<&str> = strategies.iter().map(|s| s.id()).collect();
    if cli.strategy != "all" && !available.contains(&cli.strategy.as_str()) {
        anyhow::bail!(
            "unknown strategy: {} (available: {})",
            cli.strategy,
            available.join(", ")
        );
    }
    let strategies: Vec<Arc<dyn ramseynet_worker_api::SearchStrategy>> =
        strategies.into_iter().map(Arc::from).collect();

    // Build initial config if k/ell/n provided (auto-start mode)
    let initial_config = if let (Some(k), Some(ell), Some(n)) = (cli.k, cli.ell, cli.n) {
        let init_mode = match cli.init.as_str() {
            "paley" => InitMode::Paley,
            "perturbed-paley" => InitMode::PerturbedPaley,
            "random" => InitMode::Random,
            "leaderboard" => InitMode::Leaderboard,
            other => anyhow::bail!("unknown init mode: {other}"),
        };
        let num_edges = n * (n - 1) / 2;
        let noise_flips = cli
            .noise_flips
            .unwrap_or(((num_edges as f64).sqrt() / 2.0).ceil() as u32);

        Some(EngineConfig {
            k,
            ell,
            n,
            max_iters: cli.max_iters,
            no_backoff: cli.no_backoff,
            offline: cli.offline,
            sample_bias: cli.sample_bias,
            leaderboard_sample_size: cli.leaderboard_sample_size,
            collector_capacity: cli.collector_capacity,
            max_known_cids: cli.max_known_cids,
            noise_flips,
            init_mode,
            strategy_id: Some(cli.strategy.clone()),
            strategy_config: serde_json::json!({
                "beam_width": cli.beam_width,
                "max_depth": cli.max_depth,
            }),
            server_url: cli.server.clone(),
        })
    } else {
        None
    };

    // Graceful shutdown
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Ctrl+C received, shutting down...");
        let _ = shutdown_tx.send(true);
    });

    // Command channel (UI → engine)
    let (cmd_tx, cmd_rx) = mpsc::channel::<ramseynet_worker_api::WorkerCommand>(32);

    // Event channel (engine → broadcast)
    let (event_tx, mut event_fwd_rx) = mpsc::channel::<WorkerEvent>(64);
    let (event_watch_tx, event_watch_rx) = watch::channel(None::<WorkerEvent>);

    // Forward events from mpsc to watch (so multiple WS clients can subscribe)
    tokio::spawn(async move {
        while let Some(event) = event_fwd_rx.recv().await {
            let _ = event_watch_tx.send(Some(event));
        }
    });

    // Build strategy info for viz server
    let strategy_infos: Vec<ramseynet_worker_api::StrategyInfo> = strategies
        .iter()
        .map(|s| ramseynet_worker_api::StrategyInfo {
            id: s.id().to_string(),
            name: s.name().to_string(),
            params: s.config_schema(),
        })
        .collect();

    // Start viz server if requested
    let viz: Option<Arc<dyn VizBridge>> = if let Some(port) = cli.port {
        let handle = Arc::new(VizHandle::new());
        let viz_shutdown = shutdown_rx.clone();
        let viz_for_server = Arc::clone(&handle);
        let cmd_tx_for_viz = cmd_tx.clone();
        let event_rx_for_viz = event_watch_rx.clone();
        let strats_for_viz = strategy_infos.clone();
        tokio::spawn(async move {
            ramseynet_worker::viz::server::start_viz_server(
                port,
                viz_for_server,
                cmd_tx_for_viz,
                event_rx_for_viz,
                strats_for_viz,
                viz_shutdown,
            )
            .await;
        });
        info!("worker web-app at http://localhost:{port}");
        Some(Arc::new(VizBridgeImpl::new(handle)))
    } else {
        None
    };

    // Load signing key if available
    let signing_key_id = load_signing_key_id(&cli);
    if let Some(ref kid) = signing_key_id {
        info!(key_id = %kid, "signing submissions with MineGraph identity");
    } else {
        info!("no signing key — submissions will be anonymous");
    }

    run_engine(
        initial_config,
        strategies,
        viz,
        shutdown_rx,
        cmd_rx,
        event_tx,
        cli.server.clone(),
        signing_key_id,
    )
    .await?;

    Ok(())
}

/// Try to load a signing key ID from CLI flag or default config location.
fn load_signing_key_id(cli: &Cli) -> Option<String> {
    // Check explicit --signing-key flag first
    if let Some(ref path) = cli.signing_key {
        return read_key_id_from_file(path);
    }
    // Check default location: .config/minegraph/key.json in cwd
    let default_path = std::env::current_dir()
        .ok()?
        .join(".config/minegraph/key.json");
    if default_path.exists() {
        return read_key_id_from_file(default_path.to_str()?);
    }
    None
}

fn read_key_id_from_file(path: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("key_id")?.as_str().map(|s| s.to_string())
}
