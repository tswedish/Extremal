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

    info!(
        n = config.n,
        strategy = strategy.id(),
        server = %config.server_url,
        offline = config.offline,
        "engine starting"
    );

    let mut known_cids: HashSet<GraphCid> = HashSet::new();
    let mut server_cids: HashSet<GraphCid> = HashSet::new(); // CIDs known to be on server
    let mut server_graphs: Vec<String> = Vec::new();
    let mut submit_buffer: Vec<ScoredDiscovery> = Vec::new();
    let mut round: u64 = 0;
    let mut total_discoveries: u64 = 0;
    let mut total_submitted: u64 = 0;
    let mut total_admitted: u64 = 0;
    let mut total_skipped: u64 = 0;
    let mut cid_sync_cursor: Option<String> = None;
    let mut threshold_score_bytes: Option<Vec<u8>> = None;
    let mut current_best_g6: Option<String> = None;
    let mut current_violation_score: u32 = 0;
    let mut current_goodman_gap: Option<f64> = None;
    let mut current_aut_order: Option<f64> = None;
    let mut rng = rand::rngs::SmallRng::from_entropy();
    let start_time = Instant::now();

    // Extract worker_id from metadata for heartbeat labeling, fall back to a default
    let worker_id = config
        .metadata
        .as_ref()
        .and_then(|m| m.get("worker_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let key_id = client.as_ref().and_then(|c| c.key_id()).unwrap_or_default();
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
                    threshold_score_bytes = resp
                        .threshold_score_bytes
                        .and_then(|s| hex::decode(&s).ok());
                    debug!(
                        count = resp.count,
                        capacity = resp.capacity,
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

        // ── Collect + score + dedup discoveries ─────────────
        let mut raw_discoveries: Vec<RawDiscovery> = observer.drain();
        if let Some(ref best) = result.best_graph
            && result.valid
        {
            raw_discoveries.push(RawDiscovery {
                graph: best.clone(),
                iteration: result.iterations_used,
            });
        }

        // Score locally, canonicalize, dedup
        let mut new_scored: Vec<ScoredDiscovery> = Vec::new();
        let mut round_skipped: u64 = 0;
        for discovery in &raw_discoveries {
            // Canonical form + aut_order
            let (canonical, aut_order) = canonical_form(&discovery.graph);
            let canonical_g6 = graph6::encode(&canonical);
            let cid = minegraph_graph::compute_cid(&canonical);

            // Dedup: skip if we've seen this CID
            if !known_cids.insert(cid) {
                continue;
            }

            // Skip if server already has it
            if server_cids.contains(&cid) {
                round_skipped += 1;
                continue;
            }

            // Score locally
            let histogram = CliqueHistogram::compute(&discovery.graph, max_k);
            let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
            let gap = goodman::goodman_gap(config.n, red_tri, blue_tri);
            let score = GraphScore::new(histogram, gap, aut_order, cid);
            let score_bytes = score.to_score_bytes(max_k);

            // Threshold gate: skip if can't beat the worst on the leaderboard
            if let Some(ref threshold) = threshold_score_bytes
                && score_bytes.as_slice() >= threshold.as_slice()
            {
                round_skipped += 1;
                continue;
            }

            new_scored.push(ScoredDiscovery {
                graph: discovery.graph.clone(),
                canonical_graph6: canonical_g6,
                cid,
                score,
            });
        }

        let new_unique = new_scored.len();
        total_discoveries += new_unique as u64;
        total_skipped += round_skipped;

        // Update current best for heartbeat visualization
        if let Some(ref best) = result.best_graph {
            current_best_g6 = Some(graph6::encode(best));
            // Use the first scored discovery's data if available, else basic info
            if let Some(scored) = new_scored.first() {
                current_goodman_gap = Some(scored.score.goodman_gap as f64);
                current_aut_order = Some(scored.score.aut_order);
                current_violation_score = 0; // scored discoveries are valid
            }
        }
        if !result.valid {
            current_violation_score = 1; // non-zero indicates not valid
        }

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

            // Cap buffer
            if submit_buffer.len() > 500 {
                submit_buffer.drain(..submit_buffer.len() - 200);
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
            skipped = round_skipped,
            submitted = round_submitted,
            admitted = round_admitted,
            buffered = submit_buffer.len(),
            valid = result.valid,
            ms = round_ms,
            total_discoveries,
            total_admitted,
            "round complete"
        );

        // ── Heartbeat (every 5 rounds) ──────────────────────
        if !config.offline
            && round.is_multiple_of(5)
            && let Some(ref client) = client
        {
            let heartbeat = serde_json::json!({
                "worker_id": worker_id,
                "key_id": key_id,
                "strategy": strategy.id(),
                "metadata": config.metadata,
                "n": config.n,
                "stats": {
                    "round": round,
                    "total_discoveries": total_discoveries,
                    "total_submitted": total_submitted,
                    "total_admitted": total_admitted,
                    "buffered": submit_buffer.len(),
                    "last_round_ms": round_ms,
                    "new_unique_last_round": new_unique,
                    "uptime_secs": start_time.elapsed().as_secs(),
                    "current_graph6": current_best_g6,
                    "violation_score": current_violation_score,
                    "goodman_gap": current_goodman_gap,
                    "aut_order": current_aut_order,
                },
                "last_seen": chrono::Utc::now().to_rfc3339(),
            });
            client.heartbeat(&heartbeat).await;
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
        total_discoveries, total_submitted, total_admitted, total_skipped, "engine stopped"
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
