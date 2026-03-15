//! Worker engine: main orchestration loop with state machine.
//!
//! Supports idle/searching/paused states controlled via commands from the
//! worker web-app. Coordinates search strategies, leaderboard sync, and
//! the submission pipeline.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ramseynet_graph::{compute_cid, rgxf, AdjacencyMatrix};
use ramseynet_types::GraphCid;
use ramseynet_verifier::scoring::{compute_score_canonical, GraphScore};
use ramseynet_worker_api::{
    EngineConfigPatch, ProgressInfo, SearchJob, SearchObserver, SearchStrategy, StrategyInfo,
    WorkerCommand, WorkerEvent, WorkerState, WorkerStatus,
};
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};

use crate::client::ServerClient;
use crate::error::WorkerError;
use crate::init::{self, InitMode};
use crate::VizBridge;

/// Configuration for the worker engine.
pub struct EngineConfig {
    pub k: u32,
    pub ell: u32,
    pub n: u32,
    pub max_iters: u64,
    pub no_backoff: bool,
    pub offline: bool,
    pub sample_bias: f64,
    pub leaderboard_sample_size: u32,
    pub collector_capacity: usize,
    pub max_known_cids: usize,
    pub noise_flips: u32,
    pub init_mode: InitMode,
    pub strategy_config: serde_json::Value,
    pub server_url: String,
}

/// Cached admission threshold from the server.
struct AdmissionThreshold {
    worst_score: Option<GraphScore>,
}

impl AdmissionThreshold {
    fn open() -> Self {
        Self { worst_score: None }
    }

    fn from_response(resp: &crate::client::ThresholdResponse) -> Self {
        let worst_score = if resp.entry_count >= resp.capacity {
            match (
                resp.worst_tier1_max,
                resp.worst_tier1_min,
                resp.worst_goodman_gap,
                resp.worst_tier2_aut,
                resp.worst_tier3_cid.as_ref(),
            ) {
                (Some(t1_max), Some(t1_min), Some(goodman_gap), Some(t2_aut), Some(t3_cid)) => {
                    match GraphCid::from_hex(t3_cid) {
                        Ok(cid) => Some(GraphScore::new(
                            0, 0, 0, t1_max, t1_min, goodman_gap, 0, t2_aut, cid,
                        )),
                        Err(_) => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        Self { worst_score }
    }

    fn would_admit(&self, score: &GraphScore) -> bool {
        match &self.worst_score {
            None => true,
            Some(worst) => score < worst,
        }
    }
}

/// Known CID set for cross-round deduplication.
#[derive(Clone, Default)]
pub struct KnownCids {
    inner: std::collections::HashSet<GraphCid>,
}

impl KnownCids {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_from_hex(&mut self, cids: &[String]) {
        for hex in cids {
            if let Ok(cid) = GraphCid::from_hex(hex) {
                self.inner.insert(cid);
            }
        }
    }

    pub fn insert(&mut self, cid: GraphCid) {
        self.inner.insert(cid);
    }

    pub fn insert_hex(&mut self, hex: &str) {
        if let Ok(cid) = GraphCid::from_hex(hex) {
            self.inner.insert(cid);
        }
    }

    pub fn contains(&self, cid: &GraphCid) -> bool {
        self.inner.contains(cid)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn snapshot_trimmed(&self, max: usize) -> std::collections::HashSet<GraphCid> {
        if self.inner.len() <= max {
            self.inner.clone()
        } else {
            self.inner.iter().take(max).cloned().collect()
        }
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

/// A scored discovery in the local pool.
struct LocalDiscovery {
    graph: AdjacencyMatrix,
    score: GraphScore,
    cid: GraphCid,
}

/// Shared buffer for mid-search discovery streaming.
/// The observer pushes raw discoveries; the engine drains periodically.
type DiscoveryBuffer = Arc<std::sync::Mutex<Vec<ramseynet_worker_api::RawDiscovery>>>;

/// Observer that forwards progress to the viz bridge, streams discoveries
/// to a shared buffer, and handles cancellation.
struct EngineObserver {
    cancelled: Arc<AtomicBool>,
    viz: Option<Arc<dyn VizBridge>>,
    /// Shared buffer — observer pushes, engine drains every ~30s.
    discovery_buffer: DiscoveryBuffer,
}

impl SearchObserver for EngineObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        if let Some(ref v) = self.viz {
            v.on_progress(&info.graph, info);
        }
    }

    fn on_discovery(&self, discovery: &ramseynet_worker_api::RawDiscovery) {
        let mut buf = self.discovery_buffer.lock().unwrap();
        buf.push(discovery.clone());
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// The worker engine with state machine (idle/searching/paused).
pub struct WorkerEngine;

impl WorkerEngine {
    /// Run the engine event loop. Processes commands and search rounds.
    ///
    /// If `initial_config` is `Some`, auto-starts searching. Otherwise
    /// starts in idle state waiting for a Start command from the UI.
    pub async fn run(
        initial_config: Option<EngineConfig>,
        strategies: Vec<Arc<dyn SearchStrategy>>,
        viz: Option<Arc<dyn VizBridge>>,
        mut shutdown: watch::Receiver<bool>,
        mut cmd_rx: mpsc::Receiver<WorkerCommand>,
        event_tx: mpsc::Sender<WorkerEvent>,
    ) -> Result<(), WorkerError> {
        let mut rng = SmallRng::from_entropy();
        let mut pool_rng = SmallRng::from_entropy();

        // ── Mutable search state ────────────────────────────────
        let mut state = WorkerState::Idle;
        let mut config: Option<EngineConfig> = None;
        let mut client: Option<ServerClient> = None;
        let mut known = KnownCids::new();
        let mut threshold = AdmissionThreshold::open();
        let mut cid_sync_cursor: Option<String> = None;
        let mut leaderboard_total: u32 = 0;
        let mut server_pool: Vec<AdjacencyMatrix> = Vec::new();
        let mut local_pool: Vec<LocalDiscovery> = Vec::new();
        let mut round: u64 = 0;
        let mut consecutive_failures: u32 = 0;
        let mut active_strategy_id: Option<String> = None;

        // Helper to build and send status
        let send_status = |state: &WorkerState,
                           config: &Option<EngineConfig>,
                           round: u64,
                           local_pool: &[LocalDiscovery],
                           known: &KnownCids,
                           active_strategy: &Option<String>,
                           event_tx: &mpsc::Sender<WorkerEvent>| {
            let status = WorkerStatus {
                state: state.clone(),
                k: config.as_ref().map(|c| c.k),
                ell: config.as_ref().map(|c| c.ell),
                n: config.as_ref().map(|c| c.n),
                strategy: active_strategy.clone(),
                round,
                local_pool_size: local_pool.len(),
                known_cids: known.len(),
                init_mode: config.as_ref().map(|c| format!("{:?}", c.init_mode)),
            };
            let _ = event_tx.try_send(WorkerEvent::Status(status));
        };

        // Send initial strategies info
        let strategy_infos: Vec<StrategyInfo> = strategies
            .iter()
            .map(|s| StrategyInfo {
                id: s.id().to_string(),
                name: s.name().to_string(),
                params: s.config_schema(),
            })
            .collect();
        let _ = event_tx
            .try_send(WorkerEvent::Strategies {
                strategies: strategy_infos,
            });

        // Auto-start if initial config is provided
        if let Some(cfg) = initial_config {
            info!(
                k = cfg.k, ell = cfg.ell, n = cfg.n,
                "auto-starting search from CLI args"
            );
            if !cfg.offline {
                client = Some(ServerClient::new(&cfg.server_url));
            }
            active_strategy_id = Some("tree".to_string());
            config = Some(cfg);
            state = WorkerState::Searching;
            send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
        } else {
            info!("starting in idle mode — waiting for commands");
            send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
        }

        loop {
            if *shutdown.borrow() {
                info!("shutdown signal received, exiting");
                return Ok(());
            }

            match state {
                WorkerState::Idle => {
                    // Wait for a command or shutdown
                    tokio::select! {
                        Some(cmd) = cmd_rx.recv() => {
                            match cmd {
                                WorkerCommand::Start { k, ell, n, config: patch } => {
                                    info!(k, ell, n, "received start command");
                                    let cfg = build_config(k, ell, n, &patch);
                                    if !cfg.offline {
                                        client = Some(ServerClient::new(&cfg.server_url));
                                    } else {
                                        client = None;
                                    }
                                    // Determine which strategy to use
                                    active_strategy_id = patch.strategy.or_else(|| {
                                        strategies.first().map(|s| s.id().to_string())
                                    });
                                    // Clear state for new search
                                    known.clear();
                                    local_pool.clear();
                                    threshold = AdmissionThreshold::open();
                                    cid_sync_cursor = None;
                                    leaderboard_total = 0;
                                    server_pool.clear();
                                    round = 0;
                                    consecutive_failures = 0;
                                    config = Some(cfg);
                                    state = WorkerState::Searching;
                                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                }
                                WorkerCommand::Status => {
                                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                }
                                _ => {
                                    let _ = event_tx.try_send(WorkerEvent::Error {
                                        message: format!("cannot {:?} in idle state", cmd),
                                    });
                                }
                            }
                        }
                        _ = shutdown.changed() => {
                            info!("shutdown signal received");
                            return Ok(());
                        }
                    }
                }

                WorkerState::Paused => {
                    // Wait for resume, stop, or shutdown
                    tokio::select! {
                        Some(cmd) = cmd_rx.recv() => {
                            match cmd {
                                WorkerCommand::Resume => {
                                    info!("resuming search");
                                    state = WorkerState::Searching;
                                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                }
                                WorkerCommand::Stop => {
                                    info!("stopping search (from paused)");
                                    state = WorkerState::Idle;
                                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                }
                                WorkerCommand::Status => {
                                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                }
                                _ => {
                                    let _ = event_tx.try_send(WorkerEvent::Error {
                                        message: format!("cannot {:?} in paused state", cmd),
                                    });
                                }
                            }
                        }
                        _ = shutdown.changed() => {
                            info!("shutdown signal received");
                            return Ok(());
                        }
                    }
                }

                WorkerState::Searching => {
                    let cfg = config.as_ref().unwrap();
                    let k = cfg.k;
                    let ell = cfg.ell;
                    let target_n = cfg.n;
                    let is_online = !cfg.offline && client.is_some();
                    let use_server_pool = matches!(cfg.init_mode, InitMode::Leaderboard);
                    let local_pool_capacity = cfg.collector_capacity.max(100);

                    // ── Check for commands before round ──────────────
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            WorkerCommand::Pause => {
                                info!("pausing search");
                                state = WorkerState::Paused;
                                send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                            }
                            WorkerCommand::Stop => {
                                info!("stopping search");
                                state = WorkerState::Idle;
                                send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                            }
                            WorkerCommand::Status => {
                                send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                            }
                            _ => {}
                        }
                    }
                    if state != WorkerState::Searching {
                        continue; // state changed by command
                    }

                    round += 1;

                    // ── Sync with server (online only) ───────────────
                    if is_online {
                        let cl = client.as_ref().unwrap();
                        match cl.get_threshold(k, ell, target_n).await {
                            Ok(resp) => {
                                info!(
                                    k, ell, target_n,
                                    entries = resp.entry_count,
                                    capacity = resp.capacity,
                                    worst_t1 = ?resp.worst_tier1_max,
                                    "fetched leaderboard threshold"
                                );
                                leaderboard_total = resp.entry_count;
                                threshold = AdmissionThreshold::from_response(&resp);
                            }
                            Err(e) => warn!("failed to fetch threshold: {e}"),
                        }

                        match cl
                            .get_leaderboard_cids_since(k, ell, target_n, cid_sync_cursor.as_deref())
                            .await
                        {
                            Ok(resp) => {
                                if !resp.cids.is_empty() {
                                    known.add_from_hex(&resp.cids);
                                }
                                if let Some(ref ts) = resp.last_updated {
                                    cid_sync_cursor = Some(ts.clone());
                                }
                                info!(
                                    known = known.len(), new_cids = resp.cids.len(),
                                    total = resp.total, "synced leaderboard CIDs"
                                );
                            }
                            Err(e) => warn!("failed to sync leaderboard CIDs: {e}"),
                        }

                        if use_server_pool {
                            let max_offset = leaderboard_total.saturating_sub(cfg.leaderboard_sample_size);
                            let offset = if max_offset == 0 || cfg.sample_bias >= 1.0 {
                                0
                            } else {
                                let u: f64 = pool_rng.gen();
                                let biased = u.powf(1.0 / (1.0 - cfg.sample_bias * 0.95));
                                (biased * max_offset as f64) as u32
                            };
                            match cl
                                .get_leaderboard_graphs(k, ell, target_n, cfg.leaderboard_sample_size, offset)
                                .await
                            {
                                Ok(rgxfs) => {
                                    server_pool = rgxfs.iter().filter_map(|r| rgxf::from_json(r).ok()).collect();
                                    info!(count = server_pool.len(), offset, "refreshed server seed pool");
                                }
                                Err(e) => warn!("failed to fetch leaderboard graphs: {e}"),
                            }
                        }
                    }

                    info!(k, ell, target_n, round, "starting search round");

                    // ── Pick strategy ────────────────────────────────
                    let strategy = if let Some(ref sid) = active_strategy_id {
                        strategies.iter().find(|s| s.id() == sid.as_str())
                    } else {
                        strategies.first()
                    };
                    let strategy = match strategy {
                        Some(s) => Arc::clone(s),
                        None => {
                            error!("no strategy available");
                            state = WorkerState::Idle;
                            send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                            continue;
                        }
                    };

                    let start = Instant::now();
                    let strategy_id = strategy.id().to_string();

                    info!(strategy = %strategy_id, target_n, max_iters = cfg.max_iters, "running search");

                    // ── Seed graph ───────────────────────────────────
                    let seed_graph = if use_server_pool {
                        init::sample_init_graph(&server_pool, cfg.sample_bias, target_n, cfg.noise_flips, &mut rng)
                    } else if !local_pool.is_empty() {
                        let local_graphs: Vec<AdjacencyMatrix> = local_pool.iter().map(|d| d.graph.clone()).collect();
                        init::sample_init_graph(&local_graphs, cfg.sample_bias, target_n, cfg.noise_flips, &mut rng)
                    } else {
                        init::make_init_graph(&cfg.init_mode, target_n, &mut rng)
                    };

                    let job = SearchJob {
                        k, ell, n: target_n,
                        max_iters: cfg.max_iters,
                        seed: rng.gen(),
                        init_graph: Some(seed_graph),
                        config: cfg.strategy_config.clone(),
                        known_cids: known.snapshot_trimmed(cfg.max_known_cids),
                        max_known_cids: cfg.max_known_cids,
                    };

                    let cancel_flag = Arc::new(AtomicBool::new(false));
                    let cancel_for_search = cancel_flag.clone();
                    let strategy_clone = Arc::clone(&strategy);
                    let viz_for_observer = viz.clone();
                    let discovery_buffer: DiscoveryBuffer =
                        Arc::new(std::sync::Mutex::new(Vec::new()));
                    let buffer_for_search = Arc::clone(&discovery_buffer);

                    let mut search_handle = tokio::task::spawn_blocking(move || {
                        let observer = EngineObserver {
                            cancelled: cancel_for_search,
                            viz: viz_for_observer,
                            discovery_buffer: buffer_for_search,
                        };
                        strategy_clone.search(&job, &observer)
                    });

                    // Wait for search, handling commands, shutdown, and periodic submission
                    let submit_interval = Duration::from_secs(30);
                    let mut search_cancelled = false;
                    let mut submit_timer = tokio::time::interval(submit_interval);
                    submit_timer.tick().await; // skip immediate first tick

                    let result = loop {
                        tokio::select! {
                            result = &mut search_handle => {
                                break result.unwrap();
                            }
                            // Commands handled immediately (not on a timer)
                            Some(cmd) = cmd_rx.recv() => {
                                match cmd {
                                    WorkerCommand::Pause | WorkerCommand::Stop => {
                                        info!("cancelling search for {:?}", cmd);
                                        cancel_flag.store(true, Ordering::Relaxed);
                                        search_cancelled = true;
                                        if matches!(cmd, WorkerCommand::Pause) {
                                            state = WorkerState::Paused;
                                        } else {
                                            state = WorkerState::Idle;
                                        }
                                    }
                                    WorkerCommand::Status => {
                                        send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                                    }
                                    _ => {}
                                }
                            }
                            // Shutdown handled immediately
                            _ = shutdown.changed() => {
                                if *shutdown.borrow() {
                                    cancel_flag.store(true, Ordering::Relaxed);
                                    search_cancelled = true;
                                }
                            }
                            // Periodic mid-search submission (every 30s)
                            _ = submit_timer.tick() => {
                                let drained: Vec<ramseynet_worker_api::RawDiscovery> = {
                                    let mut buf = discovery_buffer.lock().unwrap();
                                    std::mem::take(&mut *buf)
                                };
                                if !drained.is_empty() {
                                    let batch = score_and_dedup(
                                        &drained, &mut known, viz.as_ref(),
                                        target_n, &strategy_id,
                                    );
                                    feed_local_pool(&batch, &mut local_pool, local_pool_capacity, use_server_pool);
                                    if is_online && !batch.is_empty() {
                                        let cl = client.as_ref().unwrap();
                                        let (submitted, admitted, skipped) = submit_batch(
                                            cl, &batch, &threshold, &mut known,
                                            k, ell, target_n,
                                        ).await;
                                        info!(submitted, admitted, skipped, "periodic submission batch");
                                    }
                                }
                            }
                        }
                    };

                    let elapsed = start.elapsed();

                    if search_cancelled {
                        info!(
                            strategy = %strategy_id,
                            iterations = result.iterations_used,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "search interrupted"
                        );
                        send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                        if *shutdown.borrow() {
                            return Ok(());
                        }
                        continue;
                    }

                    // ── Final batch: drain any remaining buffered discoveries + result ──
                    let remaining: Vec<ramseynet_worker_api::RawDiscovery> = {
                        let mut buf = discovery_buffer.lock().unwrap();
                        std::mem::take(&mut *buf)
                    };
                    // Combine buffered (not yet processed) with any in result.discoveries
                    // that weren't streamed (e.g., the final best graph)
                    let mut final_raws = remaining;
                    if result.valid {
                        if let Some(ref best) = result.best_graph {
                            final_raws.push(ramseynet_worker_api::RawDiscovery {
                                graph: best.clone(),
                                iteration: result.iterations_used,
                            });
                        }
                    }

                    let scored = score_and_dedup(
                        &final_raws, &mut known, viz.as_ref(),
                        target_n, &strategy_id,
                    );
                    feed_local_pool(&scored, &mut local_pool, local_pool_capacity, use_server_pool);

                    // ── Log results ──────────────────────────────────
                    if !scored.is_empty() {
                        info!(strategy = %strategy_id, target_n, iterations = result.iterations_used,
                            elapsed_ms = elapsed.as_millis() as u64, discoveries = scored.len(),
                            local_pool = local_pool.len(), "search completed with discoveries");
                    } else if result.valid {
                        info!(strategy = %strategy_id, target_n, iterations = result.iterations_used,
                            elapsed_ms = elapsed.as_millis() as u64, "found valid graph (all duplicates)");
                    } else {
                        warn!(strategy = %strategy_id, target_n, iterations = result.iterations_used,
                            elapsed_ms = elapsed.as_millis() as u64, "no valid graph found");
                    }

                    // ── Submit to server ─────────────────────────────
                    if is_online && !scored.is_empty() {
                        let cl = client.as_ref().unwrap();
                        let (submitted, admitted, skipped) = submit_batch(
                            cl, &scored, &threshold, &mut known, k, ell, target_n,
                        ).await;
                        if submitted > 0 || skipped > 0 {
                            info!(submitted, admitted, skipped, "final submission batch");
                        }
                        if submitted > 0 { consecutive_failures = 0; }
                    } else if !scored.is_empty() {
                        info!(discoveries = scored.len(), "discoveries found (offline, not submitted)");
                    }

                    // ── Backoff on failure ───────────────────────────
                    if scored.is_empty() && !result.valid {
                        consecutive_failures += 1;
                        if !cfg.no_backoff {
                            let backoff_secs = (2u64.pow(consecutive_failures.min(5))).min(60);
                            warn!(consecutive_failures, backoff_secs, "no discoveries, backing off");
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                                _ = shutdown.changed() => { return Ok(()); }
                                Some(cmd) = cmd_rx.recv() => {
                                    match cmd {
                                        WorkerCommand::Pause => { state = WorkerState::Paused; }
                                        WorkerCommand::Stop => { state = WorkerState::Idle; }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    } else {
                        consecutive_failures = 0;
                    }

                    send_status(&state, &config, round, &local_pool, &known, &active_strategy_id, &event_tx);
                }
            }
        }
    }
}

/// Score raw discoveries, deduplicate by canonical CID, and forward to viz.
fn score_and_dedup(
    raws: &[ramseynet_worker_api::RawDiscovery],
    known: &mut KnownCids,
    viz: Option<&Arc<dyn VizBridge>>,
    target_n: u32,
    strategy_id: &str,
) -> Vec<LocalDiscovery> {
    let mut scored = Vec::new();
    for raw in raws {
        let sr = compute_score_canonical(&raw.graph);
        let canonical_cid = compute_cid(&sr.canonical_graph);
        if known.contains(&canonical_cid) {
            continue;
        }
        known.insert(canonical_cid.clone());
        if let Some(v) = viz {
            v.on_discovery(
                &sr.canonical_graph,
                target_n,
                strategy_id,
                raw.iteration,
                sr.score.clone(),
            );
        }
        scored.push(LocalDiscovery {
            graph: sr.canonical_graph,
            score: sr.score,
            cid: canonical_cid,
        });
    }
    scored.sort_by(|a, b| a.score.cmp(&b.score));
    scored
}

/// Insert scored discoveries into the local self-learning pool.
fn feed_local_pool(
    scored: &[LocalDiscovery],
    local_pool: &mut Vec<LocalDiscovery>,
    capacity: usize,
    use_server_pool: bool,
) {
    if use_server_pool {
        return;
    }
    for discovery in scored {
        let dominated = local_pool.len() >= capacity
            && local_pool
                .last()
                .map(|w| discovery.score >= w.score)
                .unwrap_or(false);
        if dominated {
            continue;
        }
        if local_pool.iter().any(|d| d.cid == discovery.cid) {
            continue;
        }
        let pos = local_pool
            .binary_search_by(|d| d.score.cmp(&discovery.score))
            .unwrap_or_else(|p| p);
        local_pool.insert(
            pos,
            LocalDiscovery {
                graph: discovery.graph.clone(),
                score: discovery.score.clone(),
                cid: discovery.cid.clone(),
            },
        );
        if local_pool.len() > capacity {
            local_pool.pop();
        }
    }
}

/// Submit a batch of scored discoveries to the server.
async fn submit_batch(
    client: &ServerClient,
    scored: &[LocalDiscovery],
    threshold: &AdmissionThreshold,
    known: &mut KnownCids,
    k: u32,
    ell: u32,
    n: u32,
) -> (usize, usize, usize) {
    let mut submitted = 0usize;
    let mut admitted = 0usize;
    let mut skipped = 0usize;

    for discovery in scored {
        if !threshold.would_admit(&discovery.score) {
            debug!(
                graph_cid = %discovery.cid.to_hex(),
                "skipping — below threshold"
            );
            skipped += 1;
            continue;
        }
        let rgxf_json = rgxf::to_json(&discovery.graph);
        match client.submit(k, ell, n, rgxf_json).await {
            Ok(resp) => {
                let was_admitted = resp.admitted.unwrap_or(false);
                info!(
                    graph_cid = %resp.graph_cid, verdict = %resp.verdict,
                    admitted = was_admitted, rank = ?resp.rank, "submitted graph"
                );
                known.insert_hex(&resp.graph_cid);
                submitted += 1;
                if was_admitted {
                    admitted += 1;
                    info!("admitted to leaderboard! rank={}", resp.rank.unwrap_or(0));
                }
            }
            Err(e) => error!(graph_cid = %discovery.cid.to_hex(), "submit failed: {e}"),
        }
    }
    (submitted, admitted, skipped)
}

/// Build an EngineConfig from a Start command's patch, using sensible defaults.
fn build_config(k: u32, ell: u32, n: u32, patch: &EngineConfigPatch) -> EngineConfig {
    let num_edges = n * (n - 1) / 2;
    let noise_flips = patch
        .noise_flips
        .unwrap_or(((num_edges as f64).sqrt() / 2.0).ceil() as u32);

    let init_mode = match patch.init_mode.as_deref() {
        Some("paley") => InitMode::Paley,
        Some("random") => InitMode::Random,
        Some("leaderboard") => InitMode::Leaderboard,
        _ => InitMode::PerturbedPaley,
    };

    EngineConfig {
        k,
        ell,
        n,
        max_iters: patch.max_iters.unwrap_or(100_000),
        no_backoff: patch.no_backoff.unwrap_or(false),
        offline: patch.offline.unwrap_or(false),
        sample_bias: patch.sample_bias.unwrap_or(0.5),
        leaderboard_sample_size: 100,
        collector_capacity: 1000,
        max_known_cids: 10_000,
        noise_flips,
        init_mode,
        strategy_config: patch.strategy_config.clone().unwrap_or(serde_json::json!({})),
        server_url: patch.server_url.clone().unwrap_or_else(|| "http://localhost:3001".into()),
    }
}
