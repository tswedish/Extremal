//! Worker engine: the main search round loop.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use minegraph_graph::{AdjacencyMatrix, graph6};
use minegraph_types::GraphCid;
use minegraph_worker_api::{ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchStrategy};
use rand::{Rng, SeedableRng};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::client::ServerClient;

/// Configuration for the worker engine.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Target vertex count (leaderboard index).
    pub n: u32,
    /// Maximum iterations per search round.
    pub max_iters: u64,
    /// Server URL.
    pub server_url: String,
    /// Strategy ID to use.
    pub strategy_id: String,
    /// Strategy-specific config JSON (beam_width, max_depth, target_k, etc.).
    pub strategy_config: serde_json::Value,
    /// Sample bias for leaderboard seeding (0.0 = uniform, 1.0 = always top).
    pub sample_bias: f64,
    /// Number of leaderboard graphs to fetch for seeding.
    pub leaderboard_sample_size: u32,
    /// Max known CIDs to track.
    pub max_known_cids: usize,
    /// Run without server (local search only).
    pub offline: bool,
    /// Noise flips to apply to seed graphs.
    pub noise_flips: u32,
    /// Worker metadata (commit hash, worker ID, etc.).
    pub metadata: Option<serde_json::Value>,
}

// ── Discovery-collecting observer ───────────────────────────

/// Observer that collects discoveries in a thread-safe buffer.
struct CollectingObserver {
    discoveries: Mutex<Vec<RawDiscovery>>,
}

impl CollectingObserver {
    fn new() -> Self {
        Self {
            discoveries: Mutex::new(Vec::new()),
        }
    }

    fn drain(&self) -> Vec<RawDiscovery> {
        std::mem::take(&mut *self.discoveries.lock().unwrap())
    }
}

impl SearchObserver for CollectingObserver {
    fn on_progress(&self, _info: &ProgressInfo) {
        // Could log or send to a dashboard; for now, no-op
    }

    fn on_discovery(&self, discovery: &RawDiscovery) {
        self.discoveries.lock().unwrap().push(discovery.clone());
    }
}

// ── Engine loop ─────────────────────────────────────────────

/// Run the engine loop. Blocks until shutdown signal.
pub async fn run_engine(
    config: EngineConfig,
    strategies: Vec<Arc<dyn SearchStrategy>>,
    client: Option<ServerClient>,
    shutdown: watch::Receiver<bool>,
) {
    // Find strategy
    let strategy = strategies
        .iter()
        .find(|s| s.id() == config.strategy_id)
        .cloned()
        .unwrap_or_else(|| {
            warn!(
                "strategy '{}' not found, using first available",
                config.strategy_id
            );
            strategies[0].clone()
        });

    info!(
        n = config.n,
        strategy = strategy.id(),
        server = %config.server_url,
        offline = config.offline,
        "engine starting"
    );

    let mut known_cids: HashSet<GraphCid> = HashSet::new();
    let mut server_graphs: Vec<String> = Vec::new(); // graph6 strings from leaderboard
    let mut round: u64 = 0;
    let mut total_discoveries: u64 = 0;
    let mut total_submitted: u64 = 0;
    let mut total_admitted: u64 = 0;
    let mut cid_sync_cursor: Option<String> = None;
    let mut rng = rand::rngs::SmallRng::from_entropy();

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            info!("shutdown signal received");
            break;
        }

        round += 1;
        let round_start = Instant::now();

        // ── Server sync ─────────────────────────────────────
        if !config.offline
            && let Some(ref client) = client
        {
            // Sync CIDs
            match client.get_cids(config.n, cid_sync_cursor.as_deref()).await {
                Ok(resp) => {
                    let new_count = resp.cids.len();
                    for cid_hex in &resp.cids {
                        if let Ok(cid) = GraphCid::from_hex(cid_hex) {
                            known_cids.insert(cid);
                        }
                    }
                    if new_count > 0 {
                        cid_sync_cursor = Some(chrono::Utc::now().to_rfc3339());
                        debug!(new_cids = new_count, total = known_cids.len(), "CID sync");
                    }
                }
                Err(e) => warn!("CID sync failed: {e}"),
            }

            // Fetch seed graphs (periodically, not every round)
            if round == 1 || round.is_multiple_of(10) {
                match client
                    .get_graphs(config.n, config.leaderboard_sample_size, 0)
                    .await
                {
                    Ok(graphs) => {
                        if !graphs.is_empty() {
                            debug!(count = graphs.len(), "fetched leaderboard graphs");
                            server_graphs = graphs;
                        }
                    }
                    Err(e) => warn!("graph fetch failed: {e}"),
                }
            }
        }

        // ── Seed graph ──────────────────────────────────────
        let init_graph = pick_seed(
            &server_graphs,
            config.n,
            config.sample_bias,
            config.noise_flips,
            &mut rng,
        );

        // ── Build job ───────────────────────────────────────
        let job = SearchJob {
            n: config.n,
            max_iters: config.max_iters,
            seed: rng.r#gen(),
            init_graph,
            config: config.strategy_config.clone(),
            known_cids: known_cids.clone(),
            max_known_cids: config.max_known_cids,
            carry_state: None,
        };

        // ── Run search (blocking) with collecting observer ──
        let strategy_clone = strategy.clone();
        let observer = Arc::new(CollectingObserver::new());
        let observer_clone = observer.clone();
        let result = tokio::task::spawn_blocking(move || {
            strategy_clone.search(&job, observer_clone.as_ref())
        })
        .await;

        let result = match result {
            Ok(r) => r,
            Err(e) => {
                error!("search task panicked: {e}");
                continue;
            }
        };

        // Collect all discoveries: from observer callbacks + result.best_graph
        let mut discoveries_to_submit: Vec<RawDiscovery> = observer.drain();

        // Also add the best valid graph if not already collected
        if let Some(ref best) = result.best_graph
            && result.valid
        {
            let cid = minegraph_graph::compute_cid(best);
            if known_cids.insert(cid) {
                discoveries_to_submit.push(RawDiscovery {
                    graph: best.clone(),
                    iteration: result.iterations_used,
                });
            }
        }

        total_discoveries += discoveries_to_submit.len() as u64;

        // ── Submit discoveries ──────────────────────────────
        let mut round_submitted = 0u64;
        let mut round_admitted = 0u64;

        if !config.offline
            && let Some(ref client) = client
        {
            for discovery in &discoveries_to_submit {
                let g6 = graph6::encode(&discovery.graph);
                match client.submit(config.n, &g6, config.metadata.as_ref()).await {
                    Ok(resp) => {
                        round_submitted += 1;
                        if resp.admitted {
                            round_admitted += 1;
                            if let Some(rank) = resp.rank {
                                info!(
                                    cid = %resp.cid,
                                    rank,
                                    "admitted to leaderboard"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        debug!("submit failed: {e}");
                    }
                }
            }
        }

        total_submitted += round_submitted;
        total_admitted += round_admitted;

        let round_elapsed = round_start.elapsed();
        info!(
            round,
            iters = result.iterations_used,
            discoveries = discoveries_to_submit.len(),
            submitted = round_submitted,
            admitted = round_admitted,
            valid = result.valid,
            ms = round_elapsed.as_millis() as u64,
            total_discoveries,
            total_admitted,
            "round complete"
        );

        // Trim known CIDs if too large
        if known_cids.len() > config.max_known_cids * 2 {
            let target = config.max_known_cids;
            let drain: Vec<_> = known_cids
                .iter()
                .take(known_cids.len() - target)
                .copied()
                .collect();
            for cid in drain {
                known_cids.remove(&cid);
            }
        }

        // Check shutdown again before next round
        if shutdown.has_changed().unwrap_or(false) && *shutdown.borrow() {
            info!("shutdown signal received after round");
            break;
        }
    }

    info!(
        rounds = round,
        total_discoveries, total_submitted, total_admitted, "engine stopped"
    );
}

/// Pick a seed graph from the leaderboard pool or generate a Paley graph.
fn pick_seed(
    server_graphs: &[String],
    n: u32,
    sample_bias: f64,
    noise_flips: u32,
    rng: &mut impl Rng,
) -> Option<AdjacencyMatrix> {
    // Try leaderboard graphs first
    if !server_graphs.is_empty() {
        // Biased sampling: sample_bias controls how much we prefer top-ranked.
        let idx = if sample_bias > 0.0 && server_graphs.len() > 1 {
            let u: f64 = rng.r#gen();
            let biased = u.powf(1.0 / (1.0 - sample_bias + 0.01));
            let i = (biased * server_graphs.len() as f64) as usize;
            i.min(server_graphs.len() - 1)
        } else {
            rng.gen_range(0..server_graphs.len())
        };

        let g6 = &server_graphs[idx];
        if let Ok(mut matrix) = graph6::decode(g6) {
            if noise_flips > 0 {
                minegraph_strategies::init::perturb(&mut matrix, noise_flips, rng);
            }
            return Some(matrix);
        }
    }

    // Fallback: Paley graph (much better seed than random for Ramsey search)
    let mut seed = minegraph_strategies::init::paley_graph(n);
    if noise_flips > 0 {
        minegraph_strategies::init::perturb(&mut seed, noise_flips, rng);
    }
    Some(seed)
}
