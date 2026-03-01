use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tokio::sync::watch;
use tracing::info;

use ramseynet_search::annealing::AnnealingSearcher;
use ramseynet_search::client::ServerClient;
use ramseynet_search::greedy::GreedySearcher;
use ramseynet_search::init::InitStrategy;
use ramseynet_search::local_search::LocalSearcher;
use ramseynet_search::search::Searcher;
use ramseynet_search::viz::VizHandle;
use ramseynet_search::worker::{run_worker, WorkerConfig};

#[derive(Parser)]
#[command(name = "ramseynet-search", about = "RamseyNet search worker")]
struct Cli {
    /// Server URL (e.g. http://localhost:3001)
    #[arg(long, default_value = "http://localhost:3001")]
    server: String,

    /// Challenge ID (e.g. ramsey:3:3:v1)
    #[arg(long)]
    challenge: String,

    /// Search strategy: greedy, local, annealing, or all
    #[arg(long, default_value = "all")]
    strategy: String,

    /// Starting vertex count (overrides server record)
    #[arg(long)]
    start_n: Option<u32>,

    /// Maximum iterations per search attempt
    #[arg(long, default_value = "100000")]
    max_iters: u64,

    /// Tabu tenure for local search
    #[arg(long, default_value = "10")]
    tabu_tenure: u32,

    /// Initial temperature for simulated annealing
    #[arg(long, default_value = "2.0")]
    initial_temp: f64,

    /// Cooling rate for simulated annealing
    #[arg(long, default_value = "0.9995")]
    cooling_rate: f64,

    /// Port for live visualization web server
    #[arg(long)]
    viz_port: Option<u16>,

    /// Disable backoff delay between failed search rounds
    #[arg(long)]
    no_backoff: bool,

    /// Offline mode: search continuously without a server
    #[arg(long)]
    offline: bool,

    /// Graph initialization: paley (default), perturbed-paley, random, balanced
    #[arg(long, default_value = "perturbed-paley")]
    init: String,
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
        challenge = %cli.challenge,
        strategy = %cli.strategy,
        "starting ramseynet search worker"
    );

    let client = ServerClient::new(&cli.server);

    let init_strategy = match cli.init.as_str() {
        "paley" => InitStrategy::Paley,
        "perturbed-paley" => InitStrategy::PerturbedPaley { flip_fraction: 0.05 },
        "random" => InitStrategy::Random,
        "balanced" => InitStrategy::BalancedRandom { density: 0.5 },
        other => anyhow::bail!("unknown init strategy: {other} (use paley, perturbed-paley, random, balanced)"),
    };

    let searchers: Vec<Box<dyn Searcher>> = match cli.strategy.as_str() {
        "greedy" => vec![Box::new(GreedySearcher)],
        "local" => vec![Box::new(LocalSearcher {
            tabu_tenure: cli.tabu_tenure,
            init_strategy: init_strategy.clone(),
        })],
        "annealing" => vec![Box::new(AnnealingSearcher {
            initial_temp: cli.initial_temp,
            cooling_rate: cli.cooling_rate,
            init_strategy: init_strategy.clone(),
        })],
        "all" => vec![
            Box::new(GreedySearcher),
            Box::new(LocalSearcher {
                tabu_tenure: cli.tabu_tenure,
                init_strategy: init_strategy.clone(),
            }),
            Box::new(AnnealingSearcher {
                initial_temp: cli.initial_temp,
                cooling_rate: cli.cooling_rate,
                init_strategy,
            }),
        ],
        other => anyhow::bail!("unknown strategy: {other} (use greedy, local, annealing, or all)"),
    };

    if cli.offline && cli.start_n.is_none() {
        anyhow::bail!("--offline requires --start-n to set the target vertex count");
    }

    let config = WorkerConfig {
        challenge_id: cli.challenge,
        start_n: cli.start_n,
        max_iters: cli.max_iters,
        no_backoff: cli.no_backoff,
        offline: cli.offline,
    };

    // Graceful shutdown on Ctrl+C
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Ctrl+C received, shutting down...");
        let _ = shutdown_tx.send(true);
    });

    // Start viz server if requested
    let viz_handle = if let Some(port) = cli.viz_port {
        let handle = Arc::new(VizHandle::new());
        let viz_shutdown = shutdown_rx.clone();
        let viz = Arc::clone(&handle);
        tokio::spawn(async move {
            ramseynet_search::viz::server::start_viz_server(port, viz, viz_shutdown).await;
        });
        info!("viz server at http://localhost:{port}");
        Some(handle)
    } else {
        None
    };

    run_worker(client, searchers, config, shutdown_rx, viz_handle).await?;

    Ok(())
}
