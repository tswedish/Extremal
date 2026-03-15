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
    WorkerCommand, WorkerEvent, WorkerMetrics, WorkerState, WorkerStatus,
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
    pub strategy_id: Option<String>,
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
                        Ok(cid) => Some(GraphScore::from_threshold(
                            t1_max, t1_min, goodman_gap, t2_aut, cid,
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

/// Server CID set — tracks CIDs known to be on the server leaderboard.
/// Used solely to avoid re-submitting graphs the server already has.
/// Capped at the server's reported leaderboard capacity.
#[derive(Clone, Default)]
pub struct ServerCids {
    inner: std::collections::HashSet<GraphCid>,
    cap: usize,
}

impl ServerCids {
    pub fn new() -> Self {
        Self { inner: std::collections::HashSet::new(), cap: 10_000 }
    }

    /// Update the cap to match the server's reported leaderboard capacity.
    pub fn set_cap(&mut self, cap: u32) {
        self.cap = cap as usize;
    }

    pub fn add_from_hex(&mut self, cids: &[String]) {
        for hex in cids {
            if let Ok(cid) = GraphCid::from_hex(hex) {
                self.inner.insert(cid);
            }
        }
        self.trim_if_needed();
    }

    pub fn insert(&mut self, cid: GraphCid) {
        self.inner.insert(cid);
        self.trim_if_needed();
    }

    pub fn insert_hex(&mut self, hex: &str) {
        if let Ok(cid) = GraphCid::from_hex(hex) {
            self.insert(cid);
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

    /// Snapshot for passing to strategies. Strategies use this to avoid
    /// re-exploring graphs already on the server leaderboard.
    pub fn snapshot(&self) -> std::collections::HashSet<GraphCid> {
        self.inner.clone()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    fn trim_if_needed(&mut self) {
        if self.cap > 0 && self.inner.len() > self.cap * 2 {
            // Drop roughly half (arbitrary entries — order doesn't matter
            // since the next CID sync will repopulate from server)
            let keep = self.cap;
            let to_remove: Vec<GraphCid> = self.inner.iter().skip(keep).cloned().collect();
            for cid in &to_remove {
                self.inner.remove(cid);
            }
            tracing::debug!(
                dropped = to_remove.len(),
                remaining = self.inner.len(),
                "trimmed server CID cache"
            );
        }
    }
}

/// A scored discovery in the local pool.
#[derive(Clone)]
struct LocalDiscovery {
    graph: AdjacencyMatrix,
    score: GraphScore,
    cid: GraphCid,
}

/// Discovery buffer shared between observer (push) and engine (drain).
/// Drained every few seconds during search so it stays small.
struct DiscoveryBuffer {
    items: std::sync::Mutex<Vec<ramseynet_worker_api::RawDiscovery>>,
}

impl DiscoveryBuffer {
    fn new() -> Self {
        Self {
            items: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn push(&self, discovery: ramseynet_worker_api::RawDiscovery) {
        let mut buf = self.items.lock().expect("discovery buffer lock poisoned");
        buf.push(discovery);
    }

    fn drain(&self) -> Vec<ramseynet_worker_api::RawDiscovery> {
        let mut buf = self.items.lock().expect("discovery buffer lock poisoned");
        std::mem::take(&mut *buf)
    }

    fn len(&self) -> usize {
        self.items.lock().expect("discovery buffer lock poisoned").len()
    }
}

/// Observer that forwards progress to the viz bridge, streams discoveries
/// to a bounded buffer, and handles cancellation.
struct EngineObserver {
    cancelled: Arc<AtomicBool>,
    viz: Option<Arc<dyn VizBridge>>,
    discovery_buffer: Arc<DiscoveryBuffer>,
}

impl SearchObserver for EngineObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        if let Some(ref v) = self.viz {
            v.on_progress(&info.graph, info);
        }
    }

    fn on_discovery(&self, discovery: &ramseynet_worker_api::RawDiscovery) {
        self.discovery_buffer.push(discovery.clone());
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// Run the worker engine event loop. Processes commands and search rounds.
///
/// If `initial_config` is `Some`, auto-starts searching. Otherwise
/// starts in idle state waiting for a Start command from the UI.
pub async fn run_engine(
    initial_config: Option<EngineConfig>,
    strategies: Vec<Arc<dyn SearchStrategy>>,
    viz: Option<Arc<dyn VizBridge>>,
    mut shutdown: watch::Receiver<bool>,
    mut cmd_rx: mpsc::Receiver<WorkerCommand>,
    event_tx: mpsc::Sender<WorkerEvent>,
    default_server_url: String,
) -> Result<(), WorkerError> {
        let mut rng = SmallRng::from_entropy();
        let mut pool_rng = SmallRng::from_entropy();

        // ── Mutable search state ────────────────────────────────
        let mut state = WorkerState::Idle;
        let mut config: Option<EngineConfig> = None;
        let mut client: Option<ServerClient> = None;
        let mut server_cids = ServerCids::new();
        let mut threshold = AdmissionThreshold::open();
        let mut cid_sync_cursor: Option<String> = None;
        let mut leaderboard_total: u32 = 0;
        let mut server_pool: Vec<AdjacencyMatrix> = Vec::new();
        let mut local_pool: Vec<LocalDiscovery> = Vec::new();
        let mut round: u64 = 0;
        let mut consecutive_failures: u32 = 0;
        let mut active_strategy_id: Option<String> = None;

        // Opaque strategy state carried across rounds
        let mut strategy_state: Option<Box<dyn std::any::Any + Send>> = None;

        // Runtime metrics — accumulated across rounds
        let mut metrics = WorkerMetrics::default();

        // Helper to build and send status
        let send_status = |state: &WorkerState,
                           config: &Option<EngineConfig>,
                           round: u64,
                           active_strategy: &Option<String>,
                           metrics: &WorkerMetrics,
                           event_tx: &mpsc::Sender<WorkerEvent>,
                           default_server_url: &str| {
            let server_url = config.as_ref()
                .map(|c| c.server_url.clone())
                .unwrap_or_else(|| default_server_url.to_string());
            let status = WorkerStatus {
                state: state.clone(),
                k: config.as_ref().map(|c| c.k),
                ell: config.as_ref().map(|c| c.ell),
                n: config.as_ref().map(|c| c.n),
                strategy: active_strategy.clone(),
                round,
                init_mode: config.as_ref().map(|c| format!("{:?}", c.init_mode)),
                server_url: Some(server_url),
                metrics: metrics.clone(),
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
            active_strategy_id = cfg.strategy_id.clone().or_else(|| {
                strategies.first().map(|s| s.id().to_string())
            });
            config = Some(cfg);
            state = WorkerState::Searching;
            send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
        } else {
            info!("starting in idle mode — waiting for commands");
            send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
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
                                    let cfg = build_config(k, ell, n, &patch, &default_server_url);
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
                                    server_cids.clear();
                                    local_pool.clear();
                                    strategy_state = None;
                                    threshold = AdmissionThreshold::open();
                                    cid_sync_cursor = None;
                                    leaderboard_total = 0;
                                    server_pool.clear();
                                    round = 0;
                                    consecutive_failures = 0;
                                    metrics = WorkerMetrics::default();
                                    config = Some(cfg);
                                    state = WorkerState::Searching;
                                    metrics.known_cids_count = server_cids.len();
                                    metrics.local_pool_size = local_pool.len();
                                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                }
                                WorkerCommand::Status => {
                                    metrics.known_cids_count = server_cids.len();
                                    metrics.local_pool_size = local_pool.len();
                                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                }
                                WorkerCommand::ClearKnownCids => {
                                    info!(prev = server_cids.len(), "clearing server CID cache");
                                    server_cids.clear();
                                    metrics.known_cids_count = 0;
                                }
                                WorkerCommand::ClearLocalPool => {
                                    info!(prev = local_pool.len(), "clearing local pool");
                                    local_pool.clear();
                                    metrics.local_pool_size = 0;
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
                                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                }
                                WorkerCommand::Stop => {
                                    info!("stopping search (from paused)");
                                    state = WorkerState::Idle;
                                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                }
                                WorkerCommand::Status => {
                                    metrics.known_cids_count = server_cids.len();
                                    metrics.local_pool_size = local_pool.len();
                                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                }
                                WorkerCommand::ClearKnownCids => {
                                    info!(prev = server_cids.len(), "clearing server CID cache");
                                    server_cids.clear();
                                    metrics.known_cids_count = 0;
                                }
                                WorkerCommand::ClearLocalPool => {
                                    info!(prev = local_pool.len(), "clearing local pool");
                                    local_pool.clear();
                                    metrics.local_pool_size = 0;
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
                                send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                            }
                            WorkerCommand::Stop => {
                                info!("stopping search");
                                state = WorkerState::Idle;
                                send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                            }
                            WorkerCommand::Status => {
                                metrics.known_cids_count = server_cids.len();
                                metrics.local_pool_size = local_pool.len();
                                send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                            }
                                WorkerCommand::ClearKnownCids => {
                                    info!(prev = server_cids.len(), "clearing server CID cache");
                                    server_cids.clear();
                                metrics.known_cids_count = 0;
                            }
                            WorkerCommand::ClearLocalPool => {
                                info!(prev = local_pool.len(), "clearing local pool");
                                local_pool.clear();
                                metrics.local_pool_size = 0;
                            }
                            _ => {}
                        }
                    }
                    if state != WorkerState::Searching {
                        continue;
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
                                server_cids.set_cap(resp.capacity);
                                threshold = AdmissionThreshold::from_response(&resp);
                                metrics.server_connected = true;
                                metrics.leaderboard_total = leaderboard_total;
                            }
                            Err(e) => warn!("failed to fetch threshold: {e}"),
                        }

                        match cl
                            .get_leaderboard_cids_since(k, ell, target_n, cid_sync_cursor.as_deref())
                            .await
                        {
                            Ok(resp) => {
                                if !resp.cids.is_empty() {
                                    server_cids.add_from_hex(&resp.cids);
                                }
                                if let Some(ref ts) = resp.last_updated {
                                    cid_sync_cursor = Some(ts.clone());
                                }
                                info!(
                                    server_cids = server_cids.len(), new_cids = resp.cids.len(),
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
                            send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
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
                        known_cids: server_cids.snapshot(),
                        max_known_cids: cfg.max_known_cids,
                        carry_state: strategy_state.take(),
                    };

                    let cancel_flag = Arc::new(AtomicBool::new(false));
                    let cancel_for_search = cancel_flag.clone();
                    let strategy_clone = Arc::clone(&strategy);
                    let viz_for_observer = viz.clone();
                    let discovery_buffer = Arc::new(DiscoveryBuffer::new());
                    let buffer_for_search = Arc::clone(&discovery_buffer);

                    let mut search_handle = tokio::task::spawn_blocking(move || {
                        let observer = EngineObserver {
                            cancelled: cancel_for_search,
                            viz: viz_for_observer,
                            discovery_buffer: buffer_for_search,
                        };
                        strategy_clone.search(&job, &observer)
                    });

                    // Per-round dedup set — tracks CIDs scored this round to avoid
                    // re-scoring the same canonical graph in subsequent drains.
                    let mut round_scored_cids: std::collections::HashSet<GraphCid> = std::collections::HashSet::new();

                    // Wait for search, handling commands, shutdown, and periodic scoring
                    let drain_interval = Duration::from_secs(5);
                    let mut search_cancelled = false;
                    let mut drain_timer = tokio::time::interval(drain_interval);
                    drain_timer.tick().await; // skip immediate first tick

                    let result = loop {
                        tokio::select! {
                            join_result = &mut search_handle => {
                                match join_result {
                                    Ok(result) => break Some(result),
                                    Err(e) => {
                                        error!("search strategy panicked: {e}");
                                        let _ = event_tx.try_send(WorkerEvent::Error {
                                            message: format!("strategy panicked: {e}"),
                                        });
                                        state = WorkerState::Idle;
                                        send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                                        break None;
                                    }
                                }
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
                                        send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
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
                            // Periodic drain: score discoveries and submit (every 5s)
                            _ = drain_timer.tick() => {
                                let drained = discovery_buffer.drain();
                                if !drained.is_empty() {
                                    let viz_for_scoring = viz.clone();
                                    let strategy_id_for_scoring = strategy_id.clone();
                                    let mut dedup_snapshot = round_scored_cids.clone();
                                    let batch = tokio::task::spawn_blocking(move || {
                                        score_and_dedup_with_set(
                                            &drained, &mut dedup_snapshot, viz_for_scoring.as_ref(),
                                            target_n, &strategy_id_for_scoring,
                                        )
                                    }).await.unwrap_or_default();
                                    // Merge dedup CIDs back
                                    for d in &batch {
                                        round_scored_cids.insert(d.cid.clone());
                                    }
                                    metrics.total_discoveries += batch.len() as u64;
                                    metrics.discovery_buffer_size = discovery_buffer.len();
                                    feed_local_pool(&batch, &mut local_pool, local_pool_capacity, use_server_pool);
                                    if is_online && !batch.is_empty() {
                                        let cl = client.as_ref().unwrap();
                                        let (submitted, admitted, skipped) = submit_batch(
                                            cl, &batch, &threshold, &mut server_cids,
                                            k, ell, target_n,
                                        ).await;
                                        metrics.total_submitted += submitted as u64;
                                        metrics.total_admitted += admitted as u64;
                                        metrics.total_skipped += skipped as u64;
                                        if submitted > 0 || skipped > 0 {
                                            info!(submitted, admitted, skipped, "periodic submission batch");
                                        }
                                    }
                                }
                            }
                        }
                    };

                    // Handle strategy panic (result is None)
                    let mut result = match result {
                        Some(r) => r,
                        None => continue, // state already set to Idle above
                    };

                    // Preserve strategy state for next round
                    strategy_state = result.carry_state.take();

                    let elapsed = start.elapsed();

                    if search_cancelled {
                        info!(
                            strategy = %strategy_id,
                            iterations = result.iterations_used,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "search interrupted"
                        );
                        send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                        if *shutdown.borrow() {
                            return Ok(());
                        }
                        continue;
                    }

                    // ── Final drain: score any remaining buffered discoveries ──
                    let final_raws = discovery_buffer.drain();

                    let score_start = Instant::now();
                    let scored = if !final_raws.is_empty() {
                        let viz_for_scoring = viz.clone();
                        let strategy_id_for_scoring = strategy_id.clone();
                        let mut dedup_snapshot = round_scored_cids;
                        let batch = tokio::task::spawn_blocking(move || {
                            score_and_dedup_with_set(
                                &final_raws, &mut dedup_snapshot, viz_for_scoring.as_ref(),
                                target_n, &strategy_id_for_scoring,
                            )
                        }).await.unwrap_or_default();
                        metrics.total_discoveries += batch.len() as u64;
                        batch
                    } else {
                        Vec::new()
                    };
                    feed_local_pool(&scored, &mut local_pool, local_pool_capacity, use_server_pool);
                    metrics.last_scoring_ms = score_start.elapsed().as_millis() as u64;
                    metrics.last_round_ms = elapsed.as_millis() as u64;
                    metrics.known_cids_count = server_cids.len();
                    metrics.local_pool_size = local_pool.len();
                    metrics.leaderboard_total = leaderboard_total;
                    metrics.server_connected = is_online;

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
                        let submit_start = Instant::now();
                        let cl = client.as_ref().unwrap();
                        let (submitted, admitted, skipped) = submit_batch(
                            cl, &scored, &threshold, &mut server_cids, k, ell, target_n,
                        ).await;
                        metrics.last_submit_ms = submit_start.elapsed().as_millis() as u64;
                        metrics.total_submitted += submitted as u64;
                        metrics.total_admitted += admitted as u64;
                        metrics.total_skipped += skipped as u64;
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

                    send_status(&state, &config, round, &active_strategy_id, &metrics, &event_tx, &default_server_url);
                }
            }
        }
    }

/// Score raw discoveries, deduplicate by canonical CID, and forward to viz.
///
/// Uses a per-round `seen` set to avoid re-scoring the same canonical graph.
/// This is separate from `ServerCids` which tracks server-side dedup.
fn score_and_dedup_with_set(
    raws: &[ramseynet_worker_api::RawDiscovery],
    seen: &mut std::collections::HashSet<GraphCid>,
    viz: Option<&Arc<dyn VizBridge>>,
    target_n: u32,
    strategy_id: &str,
) -> Vec<LocalDiscovery> {
    let mut scored = Vec::new();
    for raw in raws {
        let sr = compute_score_canonical(&raw.graph);
        let canonical_cid = compute_cid(&sr.canonical_graph);
        if !seen.insert(canonical_cid.clone()) {
            continue; // already scored this round
        }
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

/// Maximum concurrent submissions to the server.
const SUBMIT_CONCURRENCY: usize = 8;

/// Submit a batch of scored discoveries to the server with bounded concurrency.
async fn submit_batch(
    client: &ServerClient,
    scored: &[LocalDiscovery],
    threshold: &AdmissionThreshold,
    server_cids: &mut ServerCids,
    k: u32,
    ell: u32,
    n: u32,
) -> (usize, usize, usize) {
    let mut submitted = 0usize;
    let mut admitted = 0usize;

    // Filter to submittable discoveries (not already known, above threshold)
    let to_submit: Vec<_> = scored
        .iter()
        .filter(|d| {
            if server_cids.contains(&d.cid) {
                return false;
            }
            if !threshold.would_admit(&d.score) {
                debug!(graph_cid = %d.cid.to_hex(), "skipping — below threshold");
                return false;
            }
            true
        })
        .collect();

    let skipped = scored.len() - to_submit.len();

    // Submit with bounded concurrency using JoinSet
    let mut join_set = tokio::task::JoinSet::new();
    let mut pending = 0usize;
    let mut iter = to_submit.into_iter();

    loop {
        // Fill up to SUBMIT_CONCURRENCY concurrent tasks
        while pending < SUBMIT_CONCURRENCY {
            if let Some(discovery) = iter.next() {
                let client = client.clone();
                let rgxf_json = rgxf::to_json(&discovery.graph);
                let cid_hex = discovery.cid.to_hex();
                join_set.spawn(async move {
                    let result = client.submit(k, ell, n, rgxf_json).await;
                    (cid_hex, result)
                });
                pending += 1;
            } else {
                break;
            }
        }

        if pending == 0 {
            break;
        }

        // Wait for one task to complete
        if let Some(result) = join_set.join_next().await {
            pending -= 1;
            match result {
                Ok((_cid_hex, Ok(resp))) => {
                    let was_admitted = resp.admitted.unwrap_or(false);
                    info!(
                        graph_cid = %resp.graph_cid, verdict = %resp.verdict,
                        admitted = was_admitted, rank = ?resp.rank, "submitted graph"
                    );
                    server_cids.insert_hex(&resp.graph_cid);
                    submitted += 1;
                    if was_admitted {
                        admitted += 1;
                        info!("admitted to leaderboard! rank={}", resp.rank.unwrap_or(0));
                    }
                }
                Ok((cid_hex, Err(e))) => {
                    error!(graph_cid = %cid_hex, "submit failed: {e}");
                }
                Err(e) => {
                    error!("submit task panicked: {e}");
                }
            }
        }
    }

    (submitted, admitted, skipped)
}

/// Build an EngineConfig from a Start command's patch, using sensible defaults.
fn build_config(k: u32, ell: u32, n: u32, patch: &EngineConfigPatch, default_server_url: &str) -> EngineConfig {
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
        strategy_id: patch.strategy.clone(),
        strategy_config: patch.strategy_config.clone().unwrap_or(serde_json::json!({})),
        server_url: patch.server_url.clone().unwrap_or_else(|| default_server_url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramseynet_graph::AdjacencyMatrix;
    use ramseynet_types::GraphCid;

    fn test_cid(byte: u8) -> GraphCid {
        GraphCid([byte; 32])
    }

    fn make_score(t1_max: u64, t1_min: u64, gap: u64, aut: f64, cid_byte: u8) -> GraphScore {
        GraphScore::from_threshold(t1_max, t1_min, gap, aut, test_cid(cid_byte))
    }

    fn make_discovery(t1_max: u64, t1_min: u64, gap: u64, aut: f64, cid_byte: u8) -> LocalDiscovery {
        LocalDiscovery {
            graph: AdjacencyMatrix::new(5),
            score: make_score(t1_max, t1_min, gap, aut, cid_byte),
            cid: test_cid(cid_byte),
        }
    }

    // ── AdmissionThreshold tests ─────────────────────────

    #[test]
    fn open_threshold_admits_everything() {
        let threshold = AdmissionThreshold::open();
        let score = make_score(100, 50, 10, 1.0, 0x01);
        assert!(threshold.would_admit(&score));
    }

    #[test]
    fn full_threshold_rejects_worse() {
        let worst = make_score(10, 5, 2, 10.0, 0xFF);
        let threshold = AdmissionThreshold {
            worst_score: Some(worst),
        };

        // Better T1 — should admit
        let better = make_score(5, 3, 0, 1.0, 0x01);
        assert!(threshold.would_admit(&better));

        // Same T1 but worse gap — should reject
        let worse = make_score(10, 5, 5, 10.0, 0xFE);
        assert!(!threshold.would_admit(&worse));

        // Same everything, worse CID — should reject
        let same_but_worse_cid = make_score(10, 5, 2, 10.0, 0xFF);
        assert!(!threshold.would_admit(&same_but_worse_cid));
    }

    #[test]
    fn from_threshold_constructs_correctly() {
        let score = GraphScore::from_threshold(10, 5, 3, 120.0, test_cid(0x42));
        // Verify the tier1 tuple is set correctly
        assert_eq!(score.c_omega.max(score.c_alpha), 10);
        assert_eq!(score.c_omega.min(score.c_alpha), 5);
        assert_eq!(score.goodman_gap, 3);
        assert_eq!(score.aut_order, 120.0);
    }

    // ── ServerCids tests ──────────────────────────────────

    #[test]
    fn server_cids_basic_operations() {
        let mut cids = ServerCids::new();
        assert!(cids.is_empty());

        let cid = test_cid(0x01);
        cids.insert(cid.clone());
        assert_eq!(cids.len(), 1);
        assert!(cids.contains(&cid));
        assert!(!cids.contains(&test_cid(0x02)));
    }

    #[test]
    fn server_cids_snapshot() {
        let mut cids = ServerCids::new();
        for i in 0..20u8 {
            cids.insert(test_cid(i));
        }
        assert_eq!(cids.len(), 20);
        let snap = cids.snapshot();
        assert_eq!(snap.len(), 20);
    }

    #[test]
    fn server_cids_add_from_hex() {
        let mut cids = ServerCids::new();
        let hex = test_cid(0x42).to_hex();
        cids.add_from_hex(std::slice::from_ref(&hex));
        assert_eq!(cids.len(), 1);

        // Adding same hex again should not duplicate
        cids.add_from_hex(std::slice::from_ref(&hex));
        assert_eq!(cids.len(), 1);

        // Invalid hex should be ignored
        cids.add_from_hex(&["not_valid_hex".to_string()]);
        assert_eq!(cids.len(), 1);
    }

    #[test]
    fn server_cids_clear() {
        let mut cids = ServerCids::new();
        cids.insert(test_cid(0x01));
        cids.insert(test_cid(0x02));
        assert_eq!(cids.len(), 2);
        cids.clear();
        assert!(cids.is_empty());
    }

    #[test]
    fn server_cids_trim_at_cap() {
        let mut cids = ServerCids::new();
        cids.set_cap(10); // cap at 10, trim triggers at 2*cap = 20
        for i in 0..25u8 {
            cids.insert(test_cid(i));
        }
        // Should have trimmed to ~10 entries
        assert!(cids.len() <= 15, "should trim, got {}", cids.len());
    }

    // ── feed_local_pool tests ────────────────────────────

    #[test]
    fn local_pool_sorted_insert() {
        let mut pool: Vec<LocalDiscovery> = Vec::new();
        let d1 = make_discovery(10, 5, 2, 10.0, 0x01); // worse
        let d2 = make_discovery(5, 3, 0, 10.0, 0x02); // better T1
        let d3 = make_discovery(5, 3, 1, 10.0, 0x03); // same T1, worse gap

        feed_local_pool(std::slice::from_ref(&d1), &mut pool, 10, false);
        feed_local_pool(std::slice::from_ref(&d2), &mut pool, 10, false);
        feed_local_pool(std::slice::from_ref(&d3), &mut pool, 10, false);

        assert_eq!(pool.len(), 3);
        // Best should be first (lowest T1, lowest gap)
        assert_eq!(pool[0].cid, test_cid(0x02)); // d2: (5,3) gap=0
        assert_eq!(pool[1].cid, test_cid(0x03)); // d3: (5,3) gap=1
        assert_eq!(pool[2].cid, test_cid(0x01)); // d1: (10,5) gap=2
    }

    #[test]
    fn local_pool_capacity_enforced() {
        let mut pool: Vec<LocalDiscovery> = Vec::new();
        let cap = 3;

        for i in 0..5u8 {
            let d = make_discovery(i as u64, i as u64, 0, 1.0, i);
            feed_local_pool(std::slice::from_ref(&d), &mut pool, cap, false);
        }

        assert_eq!(pool.len(), cap);
        // Best 3 should survive (i=0, 1, 2)
        assert_eq!(pool[0].cid, test_cid(0));
        assert_eq!(pool[1].cid, test_cid(1));
        assert_eq!(pool[2].cid, test_cid(2));
    }

    #[test]
    fn local_pool_cid_dedup() {
        let mut pool: Vec<LocalDiscovery> = Vec::new();
        let d1 = make_discovery(5, 3, 0, 10.0, 0x01);
        let d2 = make_discovery(5, 3, 0, 10.0, 0x01); // same CID

        feed_local_pool(std::slice::from_ref(&d1), &mut pool, 10, false);
        feed_local_pool(std::slice::from_ref(&d2), &mut pool, 10, false);

        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn local_pool_skips_server_mode() {
        let mut pool: Vec<LocalDiscovery> = Vec::new();
        let d = make_discovery(5, 3, 0, 10.0, 0x01);

        feed_local_pool(std::slice::from_ref(&d), &mut pool, 10, true);

        assert_eq!(pool.len(), 0); // should not add
    }
}
