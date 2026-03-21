//! Worker engine: the main search round loop.
//!
//! Key optimizations (ported from RamseyNet prototype):
//! - **Threshold gate**: fetch admission threshold once per round, skip
//!   submissions that can't beat it (saves ~90%+ of network calls)
//! - **Server CID cache**: incremental CID sync, never re-submit known CIDs
//! - **Local canonical scoring**: score + canonicalize locally before deciding
//!   to submit
//! - **Batched submissions**: buffer discoveries, drain N per round
//! - **Backoff on failure**: exponential backoff when rounds produce nothing

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use minegraph_graph::{AdjacencyMatrix, graph6};
use minegraph_scoring::automorphism::canonical_form;
use minegraph_scoring::goodman;
use minegraph_scoring::histogram::CliqueHistogram;
use minegraph_scoring::score::GraphScore;
use minegraph_types::GraphCid;
use minegraph_worker_api::{ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchStrategy};
use rand::{Rng, SeedableRng};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::client::ServerClient;
use crate::dashboard::DashboardClient;
use minegraph_dashboard::protocol::WorkerMessage;

/// Configuration for the worker engine.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub n: u32,
    pub max_iters: u64,
    pub server_url: String,
    pub strategy_id: String,
    pub strategy_config: serde_json::Value,
    pub sample_bias: f64,
    pub leaderboard_sample_size: u32,
    pub max_known_cids: usize,
    pub offline: bool,
    pub noise_flips: u32,
    pub max_submissions_per_round: usize,
    pub metadata: Option<serde_json::Value>,
    /// Dashboard relay server URL (e.g. ws://localhost:4000/ws/worker).
    pub dashboard_url: Option<String>,
}

// ── Scored discovery (locally scored + canonical) ────────────

#[allow(dead_code)]
struct ScoredDiscovery {
    graph: AdjacencyMatrix,
    canonical_graph6: String,
    cid: GraphCid,
    score: GraphScore,
}

// ── Discovery-collecting observer ───────────────────────────

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
    fn on_progress(&self, _info: &ProgressInfo) {}

    fn on_discovery(&self, discovery: &RawDiscovery) {
        self.discoveries.lock().unwrap().push(discovery.clone());
    }
}

// ── Dashboard-aware observer ────────────────────────────────

/// Observer that collects discoveries AND forwards progress to the dashboard.
struct DashboardObserver {
    inner: CollectingObserver,
    dashboard: DashboardClient,
}

impl DashboardObserver {
    fn new(dashboard: DashboardClient) -> Self {
        Self {
            inner: CollectingObserver::new(),
            dashboard,
        }
    }

    fn drain(&self) -> Vec<RawDiscovery> {
        self.inner.drain()
    }
}

impl SearchObserver for DashboardObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        self.dashboard.send(WorkerMessage::Progress {
            iteration: info.iteration,
            max_iters: info.max_iters,
            violation_score: info.violation_score,
            current_graph6: graph6::encode(&info.graph),
            discoveries_so_far: info.discoveries_so_far,
        });
    }

    fn on_discovery(&self, discovery: &RawDiscovery) {
        self.inner.on_discovery(discovery);
        // Note: full scored discovery is sent after scoring in the round loop
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

    let max_k = config
        .strategy_config
        .get("target_k")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as u32;

    // Extract worker_id from metadata
    let worker_id = config
        .metadata
        .as_ref()
        .and_then(|m| m.get("worker_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let key_id = client.as_ref().and_then(|c| c.key_id()).unwrap_or_default();

    // Connect to dashboard relay if configured
    let dashboard = config.dashboard_url.as_ref().map(|url| {
        info!(url, "connecting to dashboard relay");
        DashboardClient::connect(
            url.clone(),
            WorkerMessage::Register {
                key_id: key_id.clone(),
                worker_id: worker_id.clone(),
                n: config.n,
                strategy: strategy.id().to_string(),
                metadata: config.metadata.clone(),
            },
            shutdown.clone(),
        )
    });

    info!(
        n = config.n,
        strategy = strategy.id(),
        server = %config.server_url,
        offline = config.offline,
        dashboard = config.dashboard_url.as_deref().unwrap_or("none"),
        "engine starting"
    );

    let mut known_cids: HashSet<GraphCid> = HashSet::new(); // CIDs in submit buffer or already submitted
    let mut server_cids: HashSet<GraphCid> = HashSet::new(); // CIDs confirmed on server
    let mut server_graphs: Vec<String> = Vec::new();
    let mut submit_buffer: Vec<ScoredDiscovery> = Vec::new();
    let mut round: u64 = 0;
    let mut total_discoveries: u64 = 0;
    let mut total_submitted: u64 = 0;
    let mut total_admitted: u64 = 0;
    let mut total_skipped_threshold: u64 = 0;
    let mut total_skipped_dup: u64 = 0;
    let mut total_skipped_server: u64 = 0;
    let mut cid_sync_cursor: Option<String> = None;
    let mut threshold_score_bytes: Option<Vec<u8>> = None;
    let mut leaderboard_full: bool = false;
    let mut rng = rand::rngs::SmallRng::from_entropy();

    let batch_size = if config.max_submissions_per_round > 0 {
        config.max_submissions_per_round
    } else {
        20
    };

    loop {
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
            // 1. Fetch admission threshold
            match client.get_threshold(config.n).await {
                Ok(resp) => {
                    leaderboard_full = resp.count >= resp.capacity as i64;
                    threshold_score_bytes = resp
                        .threshold_score_bytes
                        .and_then(|s| hex::decode(&s).ok());
                    debug!(
                        count = resp.count,
                        capacity = resp.capacity,
                        full = leaderboard_full,
                        has_threshold = threshold_score_bytes.is_some(),
                        "threshold sync"
                    );
                }
                Err(e) => debug!("threshold fetch failed: {e}"),
            }

            // 2. Sync CIDs (incremental)
            match client.get_cids(config.n, cid_sync_cursor.as_deref()).await {
                Ok(resp) => {
                    let new_count = resp.cids.len();
                    for cid_hex in &resp.cids {
                        if let Ok(cid) = GraphCid::from_hex(cid_hex) {
                            known_cids.insert(cid);
                            server_cids.insert(cid);
                        }
                    }
                    if new_count > 0 {
                        cid_sync_cursor = Some(chrono::Utc::now().to_rfc3339());
                        debug!(
                            new_cids = new_count,
                            known = known_cids.len(),
                            server = server_cids.len(),
                            "CID sync"
                        );
                    }
                }
                Err(e) => warn!("CID sync failed: {e}"),
            }

            // 3. Fetch seed graphs (every 10 rounds)
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

        // ── Run search ──────────────────────────────────────
        let strategy_clone = strategy.clone();

        let raw_discoveries: Vec<RawDiscovery>;
        let result;

        if let Some(ref dash) = dashboard {
            let observer = Arc::new(DashboardObserver::new(dash.clone()));
            let observer_clone = observer.clone();
            let r = tokio::task::spawn_blocking(move || {
                strategy_clone.search(&job, observer_clone.as_ref())
            })
            .await;
            result = match r {
                Ok(r) => r,
                Err(e) => {
                    error!("search task panicked: {e}");
                    continue;
                }
            };
            raw_discoveries = observer.drain();
        } else {
            let observer = Arc::new(CollectingObserver::new());
            let observer_clone = observer.clone();
            let r = tokio::task::spawn_blocking(move || {
                strategy_clone.search(&job, observer_clone.as_ref())
            })
            .await;
            result = match r {
                Ok(r) => r,
                Err(e) => {
                    error!("search task panicked: {e}");
                    continue;
                }
            };
            raw_discoveries = observer.drain();
        }

        // ── Collect + score + dedup discoveries ─────────────
        let mut raw_discoveries = raw_discoveries;
        if let Some(ref best) = result.best_graph
            && result.valid
        {
            raw_discoveries.push(RawDiscovery {
                graph: best.clone(),
                iteration: result.iterations_used,
            });
        }

        // Score locally, canonicalize, dedup, threshold-gate
        let mut new_scored: Vec<ScoredDiscovery> = Vec::new();
        let mut round_skipped_dup: u64 = 0;
        let mut round_skipped_server: u64 = 0;
        let mut round_skipped_threshold: u64 = 0;
        let mut local_pool_cids: HashSet<GraphCid> = HashSet::new();

        for discovery in &raw_discoveries {
            // Canonical form + aut_order
            let (canonical, aut_order) = canonical_form(&discovery.graph);
            let canonical_g6 = graph6::encode(&canonical);
            let cid = minegraph_graph::compute_cid(&canonical);

            // Score locally (always, even for dupes — dashboard wants all unique discoveries)
            let histogram = CliqueHistogram::compute(&discovery.graph, max_k);
            let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
            let gap = goodman::goodman_gap(config.n, red_tri, blue_tri);
            let score = GraphScore::new(histogram, gap, aut_order, cid);
            let score_bytes = score.to_score_bytes(max_k);

            // Send ALL unique scored discoveries to dashboard (for local pool visualization)
            // This feeds the rain column regardless of server dedup/threshold
            if let Some(ref dash) = dashboard
                && !local_pool_cids.contains(&cid)
            {
                local_pool_cids.insert(cid);
                dash.send(WorkerMessage::Discovery {
                    graph6: canonical_g6.clone(),
                    cid: cid.to_hex(),
                    goodman_gap: gap as f64,
                    aut_order,
                    score_hex: hex::encode(&score_bytes),
                    histogram: score
                        .histogram
                        .tiers
                        .iter()
                        .map(|t| (t.k, t.red, t.blue))
                        .collect(),
                    iteration: discovery.iteration,
                });
            }

            // Dedup for server submission pipeline
            if known_cids.contains(&cid) {
                round_skipped_dup += 1;
                continue;
            }

            if server_cids.contains(&cid) {
                round_skipped_server += 1;
                continue;
            }

            // Threshold gate: ONLY when leaderboard is full
            if leaderboard_full
                && let Some(ref threshold) = threshold_score_bytes
                && score_bytes.as_slice() >= threshold.as_slice()
            {
                round_skipped_threshold += 1;
                continue;
            }

            // Passed submission filters — mark as known and queue
            known_cids.insert(cid);

            new_scored.push(ScoredDiscovery {
                graph: discovery.graph.clone(),
                canonical_graph6: canonical_g6,
                cid,
                score,
            });
        }

        let new_unique = new_scored.len();
        total_discoveries += new_unique as u64;
        total_skipped_dup += round_skipped_dup;
        total_skipped_server += round_skipped_server;
        total_skipped_threshold += round_skipped_threshold;

        // Add to submit buffer
        submit_buffer.extend(new_scored);

        // ── Submit a batch from the buffer ──────────────────
        let mut round_submitted = 0u64;
        let mut round_admitted = 0u64;

        if !config.offline
            && let Some(ref client) = client
        {
            let count = submit_buffer.len().min(batch_size);
            let to_submit: Vec<_> = submit_buffer.drain(..count).collect();
            for discovery in &to_submit {
                let g6 = graph6::encode(&discovery.graph);
                match client.submit(config.n, &g6, config.metadata.as_ref()).await {
                    Ok(resp) => {
                        round_submitted += 1;
                        server_cids.insert(discovery.cid);
                        if resp.admitted {
                            round_admitted += 1;
                            if let Some(rank) = resp.rank {
                                info!(cid = %resp.cid, rank, "admitted");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("submit failed: {e}");
                        break;
                    }
                }
            }

            // Cap buffer — remove dropped CIDs from known_cids so they can be rediscovered
            if submit_buffer.len() > 500 {
                let dropped: Vec<_> = submit_buffer.drain(..submit_buffer.len() - 200).collect();
                for d in &dropped {
                    known_cids.remove(&d.cid);
                }
            }
        }

        total_submitted += round_submitted;
        total_admitted += round_admitted;

        let round_elapsed = round_start.elapsed();
        let round_ms = round_elapsed.as_millis() as u64;

        info!(
            round,
            iters = result.iterations_used,
            new_unique,
            skip_dup = round_skipped_dup,
            skip_srv = round_skipped_server,
            skip_thr = round_skipped_threshold,
            submitted = round_submitted,
            admitted = round_admitted,
            buffered = submit_buffer.len(),
            full = leaderboard_full,
            valid = result.valid,
            ms = round_ms,
            total_discoveries,
            total_admitted,
            "round complete"
        );

        // Send round summary to dashboard
        if let Some(ref dash) = dashboard {
            dash.send(WorkerMessage::RoundComplete {
                round,
                duration_ms: round_ms,
                discoveries: new_unique as u64,
                submitted: round_submitted,
                admitted: round_admitted,
                buffered: submit_buffer.len(),
            });
        }

        // Trim known CIDs
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
        if server_cids.len() > config.max_known_cids {
            let target = config.max_known_cids / 2;
            let drain: Vec<_> = server_cids
                .iter()
                .take(server_cids.len() - target)
                .copied()
                .collect();
            for cid in drain {
                server_cids.remove(&cid);
            }
        }

        if shutdown.has_changed().unwrap_or(false) && *shutdown.borrow() {
            info!("shutdown signal received after round");
            break;
        }
    }

    info!(
        rounds = round,
        total_discoveries,
        total_submitted,
        total_admitted,
        total_skipped_dup,
        total_skipped_server,
        total_skipped_threshold,
        "engine stopped"
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
    if !server_graphs.is_empty() {
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

    let mut seed = minegraph_strategies::init::paley_graph(n);
    if noise_flips > 0 {
        minegraph_strategies::init::perturb(&mut seed, noise_flips, rng);
    }
    Some(seed)
}
