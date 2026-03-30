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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use extremal_graph::{AdjacencyMatrix, graph6};
use extremal_identity::Identity;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::goodman;
use extremal_scoring::histogram::CliqueHistogram;
use extremal_scoring::score::GraphScore;
use extremal_types::GraphCid;
use extremal_worker_api::{
    CollectingObserver, ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob,
    SearchObserver, SearchStrategy, WorkerState,
};
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, warn};

use crate::client::ServerClient;
use crate::dashboard::DashboardClient;
use extremal_dashboard::protocol::WorkerMessage;

// ── Configuration ─────────────────────────────────────────

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

// ── Engine command channel types ──────────────────────────

/// Commands sent to the engine from the HTTP API.
pub enum EngineCommand {
    Pause,
    Resume,
    Stop,
    GetStatus {
        reply: oneshot::Sender<EngineSnapshot>,
    },
    UpdateConfig {
        patch: HashMap<String, serde_json::Value>,
        reply: oneshot::Sender<ConfigUpdateResult>,
    },
}

/// Status snapshot of the engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub state: WorkerState,
    pub round: u64,
    pub n: u32,
    pub strategy: String,
    pub worker_id: String,
    pub key_id: String,
    pub config: ConfigSnapshot,
    pub metrics: EngineMetrics,
}

/// Current configuration with values and adjustability info.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub params: Vec<ConfigParamWithValue>,
}

/// A single config parameter with its current value and metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigParamWithValue {
    pub param: ConfigParam,
    pub value: serde_json::Value,
    pub source: String,
}

/// Result of a config update request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigUpdateResult {
    pub applied: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub effective_round: u64,
}

/// Engine runtime metrics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineMetrics {
    pub total_discoveries: u64,
    pub total_submitted: u64,
    pub total_admitted: u64,
    pub submit_buffer_size: usize,
    pub known_cids_count: usize,
    pub server_cids_count: usize,
    pub last_round_ms: u64,
}

// ── Engine-level config params ────────────────────────────

/// Returns ConfigParam descriptors for engine-level adjustable params.
pub fn engine_config_params() -> Vec<ConfigParam> {
    vec![
        ConfigParam {
            name: "max_iters".into(),
            label: "Max Iterations".into(),
            description: "Iteration budget per search round".into(),
            param_type: ParamType::Int {
                min: 100,
                max: 10_000_000,
            },
            default: serde_json::json!(100_000),
            adjustable: true,
        },
        ConfigParam {
            name: "sample_bias".into(),
            label: "Sample Bias".into(),
            description: "Leaderboard seed bias (0=uniform, 1=top-biased)".into(),
            param_type: ParamType::Float { min: 0.0, max: 1.0 },
            default: serde_json::json!(0.8),
            adjustable: true,
        },
        ConfigParam {
            name: "noise_flips".into(),
            label: "Noise Flips".into(),
            description: "Random edge flips applied to seed graphs".into(),
            param_type: ParamType::Int { min: 0, max: 100 },
            default: serde_json::json!(0),
            adjustable: true,
        },
        ConfigParam {
            name: "max_submissions_per_round".into(),
            label: "Max Submissions/Round".into(),
            description: "Maximum graphs submitted per round (0=unlimited)".into(),
            param_type: ParamType::Int { min: 0, max: 1000 },
            default: serde_json::json!(20),
            adjustable: true,
        },
        ConfigParam {
            name: "n".into(),
            label: "Vertex Count".into(),
            description: "Target vertex count (cannot be changed at runtime)".into(),
            param_type: ParamType::Int { min: 3, max: 100 },
            default: serde_json::json!(25),
            adjustable: false,
        },
    ]
}

// ── Scored discovery (locally scored + canonical) ────────────

#[allow(dead_code)]
struct ScoredDiscovery {
    graph: AdjacencyMatrix,
    canonical_graph6: String,
    cid: GraphCid,
    score: GraphScore,
}

// ── Dashboard-aware observer ────────────────────────────────

/// Observer that collects discoveries AND forwards progress to the dashboard.
/// Progress events are throttled to ~4 Hz to avoid overwhelming the browser.
struct DashboardObserver {
    inner: CollectingObserver,
    dashboard: DashboardClient,
    last_progress: Mutex<Instant>,
}

/// Minimum interval between progress messages (250ms = 4 Hz).
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl DashboardObserver {
    fn new(dashboard: DashboardClient) -> Self {
        Self {
            inner: CollectingObserver::new(),
            dashboard,
            last_progress: Mutex::new(Instant::now()),
        }
    }

    fn drain(&self) -> Vec<RawDiscovery> {
        self.inner.drain()
    }
}

impl SearchObserver for DashboardObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        // Throttle: only send if enough time has passed since last progress
        let mut last = self.last_progress.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if now.duration_since(*last) < PROGRESS_INTERVAL {
            return;
        }
        *last = now;

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
    }
}

// ── Command handling ──────────────────────────────────────

/// Mutable engine stats passed to build_snapshot.
struct EngineStats {
    round: u64,
    total_discoveries: u64,
    total_submitted: u64,
    total_admitted: u64,
    submit_buffer_size: usize,
    known_cids_count: usize,
    server_cids_count: usize,
    last_round_ms: u64,
}

fn build_snapshot(
    state: WorkerState,
    config: &EngineConfig,
    strategy: &dyn SearchStrategy,
    worker_id: &str,
    key_id: &str,
    stats: &EngineStats,
) -> EngineSnapshot {
    let mut params = Vec::new();

    // Engine-level params
    for p in engine_config_params() {
        let value = match p.name.as_str() {
            "max_iters" => serde_json::json!(config.max_iters),
            "sample_bias" => serde_json::json!(config.sample_bias),
            "noise_flips" => serde_json::json!(config.noise_flips),
            "max_submissions_per_round" => serde_json::json!(config.max_submissions_per_round),
            "n" => serde_json::json!(config.n),
            _ => p.default.clone(),
        };
        params.push(ConfigParamWithValue {
            param: p,
            value,
            source: "engine".into(),
        });
    }

    // Strategy-level params
    for p in strategy.config_schema() {
        let value = config
            .strategy_config
            .get(&p.name)
            .cloned()
            .unwrap_or_else(|| p.default.clone());
        params.push(ConfigParamWithValue {
            param: p,
            value,
            source: "strategy".into(),
        });
    }

    EngineSnapshot {
        state,
        round: stats.round,
        n: config.n,
        strategy: strategy.id().to_string(),
        worker_id: worker_id.to_string(),
        key_id: key_id.to_string(),
        config: ConfigSnapshot { params },
        metrics: EngineMetrics {
            total_discoveries: stats.total_discoveries,
            total_submitted: stats.total_submitted,
            total_admitted: stats.total_admitted,
            submit_buffer_size: stats.submit_buffer_size,
            known_cids_count: stats.known_cids_count,
            server_cids_count: stats.server_cids_count,
            last_round_ms: stats.last_round_ms,
        },
    }
}

/// Validate and apply a config patch. Returns which params were applied and which had errors.
fn apply_config_patch(
    config: &mut EngineConfig,
    strategy: &dyn SearchStrategy,
    patch: HashMap<String, serde_json::Value>,
    next_round: u64,
) -> ConfigUpdateResult {
    let engine_params = engine_config_params();
    let strategy_params = strategy.config_schema();

    let mut applied = Vec::new();
    let mut errors = Vec::new();

    for (name, value) in patch {
        // Check engine params first
        if let Some(ep) = engine_params.iter().find(|p| p.name == name) {
            if !ep.adjustable {
                errors.push((name, "parameter is not adjustable at runtime".into()));
                continue;
            }
            if let Err(e) = validate_param_value(ep, &value) {
                errors.push((name, e));
                continue;
            }
            match name.as_str() {
                "max_iters" => config.max_iters = value.as_u64().unwrap(),
                "sample_bias" => config.sample_bias = value.as_f64().unwrap(),
                "noise_flips" => config.noise_flips = value.as_u64().unwrap() as u32,
                "max_submissions_per_round" => {
                    config.max_submissions_per_round = value.as_u64().unwrap() as usize
                }
                _ => {
                    errors.push((name, "unknown engine param".into()));
                    continue;
                }
            }
            applied.push(name);
            continue;
        }

        // Check strategy params
        if let Some(sp) = strategy_params.iter().find(|p| p.name == name) {
            if !sp.adjustable {
                errors.push((name, "parameter is not adjustable at runtime".into()));
                continue;
            }
            if let Err(e) = validate_param_value(sp, &value) {
                errors.push((name, e));
                continue;
            }
            if let Some(obj) = config.strategy_config.as_object_mut() {
                obj.insert(name.clone(), value);
            }
            applied.push(name);
            continue;
        }

        errors.push((name, "unknown parameter".into()));
    }

    ConfigUpdateResult {
        applied,
        errors,
        effective_round: next_round,
    }
}

fn validate_param_value(param: &ConfigParam, value: &serde_json::Value) -> Result<(), String> {
    match &param.param_type {
        ParamType::Int { min, max } => {
            let v = value
                .as_i64()
                .ok_or_else(|| format!("expected integer, got {value}"))?;
            if v < *min || v > *max {
                return Err(format!("value {v} out of range [{min}, {max}]"));
            }
        }
        ParamType::Float { min, max } => {
            let v = value
                .as_f64()
                .ok_or_else(|| format!("expected float, got {value}"))?;
            if v < *min || v > *max {
                return Err(format!("value {v} out of range [{min}, {max}]"));
            }
        }
        ParamType::Bool => {
            if !value.is_boolean() {
                return Err(format!("expected boolean, got {value}"));
            }
        }
    }
    Ok(())
}

// ── Engine loop ─────────────────────────────────────────────

/// Run the engine loop. Blocks until shutdown signal.
#[allow(clippy::too_many_arguments)]
pub async fn run_engine(
    config: EngineConfig,
    strategies: Vec<Arc<dyn SearchStrategy>>,
    client: Option<ServerClient>,
    identity: Option<Arc<Identity>>,
    mut shutdown: watch::Receiver<bool>,
    cmd_rx: Option<mpsc::Receiver<EngineCommand>>,
    snapshot_tx: Option<watch::Sender<EngineSnapshot>>,
    api_addr: Option<String>,
) {
    let mut config = config;
    let mut cmd_rx = cmd_rx;

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
            identity.clone(),
            worker_id.clone(),
            config.n,
            strategy.id().to_string(),
            config.metadata.clone(),
            api_addr,
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

    let mut engine_state = WorkerState::Searching;
    let mut known_cids: HashSet<GraphCid> = HashSet::new();
    let mut server_cids: HashSet<GraphCid> = HashSet::new();
    let mut dash_sent_cids: HashSet<GraphCid> = HashSet::new();
    let mut server_graphs: Vec<String> = Vec::new();
    let mut submit_buffer: Vec<ScoredDiscovery> = Vec::new();
    let mut total_skipped_threshold: u64 = 0;
    let mut total_skipped_dup: u64 = 0;
    let mut total_skipped_server: u64 = 0;
    let mut cid_sync_cursor: Option<String> = None;
    let mut threshold_score_bytes: Option<Vec<u8>> = None;
    let mut leaderboard_full: bool = false;
    let mut rng = rand::rngs::SmallRng::from_entropy();

    let mut stats = EngineStats {
        round: 0,
        total_discoveries: 0,
        total_submitted: 0,
        total_admitted: 0,
        submit_buffer_size: 0,
        known_cids_count: 0,
        server_cids_count: 0,
        last_round_ms: 0,
    };

    // Send initial snapshot
    if let Some(ref tx) = snapshot_tx {
        let _ = tx.send(build_snapshot(
            engine_state.clone(),
            &config,
            strategy.as_ref(),
            &worker_id,
            &key_id,
            &stats,
        ));
    }

    // Cross-round state for strategy continuity
    let mut carry_state: Option<Box<dyn std::any::Any + Send>> = None;

    loop {
        if *shutdown.borrow() {
            info!("shutdown signal received");
            break;
        }

        // ── Process pending commands ──────────────────────
        if let Some(ref mut rx) = cmd_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    EngineCommand::Pause => {
                        engine_state = WorkerState::Paused;
                        info!("engine paused (will take effect before next round)");
                    }
                    EngineCommand::Resume => {
                        engine_state = WorkerState::Searching;
                        info!("engine resumed");
                    }
                    EngineCommand::Stop => {
                        info!("stop command received");
                        engine_state = WorkerState::Idle;
                    }
                    EngineCommand::GetStatus { reply } => {
                        stats.submit_buffer_size = submit_buffer.len();
                        stats.known_cids_count = known_cids.len();
                        stats.server_cids_count = server_cids.len();
                        let snap = build_snapshot(
                            engine_state.clone(),
                            &config,
                            strategy.as_ref(),
                            &worker_id,
                            &key_id,
                            &stats,
                        );
                        let _ = reply.send(snap);
                    }
                    EngineCommand::UpdateConfig { patch, reply } => {
                        let result = apply_config_patch(
                            &mut config,
                            strategy.as_ref(),
                            patch,
                            stats.round + 1,
                        );
                        if !result.applied.is_empty() {
                            info!(applied = ?result.applied, "config updated");
                        }
                        let _ = reply.send(result);
                    }
                }
            }
        }

        // ── Handle paused state ──────────────────────────
        if engine_state == WorkerState::Paused {
            if let Some(ref mut rx) = cmd_rx {
                // Update snapshot to reflect paused state
                if let Some(ref tx) = snapshot_tx {
                    stats.submit_buffer_size = submit_buffer.len();
                    stats.known_cids_count = known_cids.len();
                    stats.server_cids_count = server_cids.len();
                    let _ = tx.send(build_snapshot(
                        engine_state.clone(),
                        &config,
                        strategy.as_ref(),
                        &worker_id,
                        &key_id,
                        &stats,
                    ));
                }

                loop {
                    tokio::select! {
                        Some(cmd) = rx.recv() => {
                            match cmd {
                                EngineCommand::Resume => {
                                    engine_state = WorkerState::Searching;
                                    info!("engine resumed");
                                    break;
                                }
                                EngineCommand::Stop => {
                                    info!("stop command received while paused");
                                    engine_state = WorkerState::Idle;
                                    break;
                                }
                                EngineCommand::GetStatus { reply } => {
                                    stats.submit_buffer_size = submit_buffer.len();
                                    stats.known_cids_count = known_cids.len();
                                    stats.server_cids_count = server_cids.len();
                                    let snap = build_snapshot(
                                        engine_state.clone(),
                                        &config,
                                        strategy.as_ref(),
                                        &worker_id,
                                        &key_id,
                                        &stats,
                                    );
                                    let _ = reply.send(snap);
                                }
                                EngineCommand::UpdateConfig { patch, reply } => {
                                    let result = apply_config_patch(
                                        &mut config,
                                        strategy.as_ref(),
                                        patch,
                                        stats.round + 1,
                                    );
                                    if !result.applied.is_empty() {
                                        info!(applied = ?result.applied, "config updated while paused");
                                    }
                                    let _ = reply.send(result);
                                }
                                EngineCommand::Pause => {} // already paused
                            }
                        }
                        _ = shutdown.changed() => {
                            if *shutdown.borrow() {
                                info!("shutdown signal while paused");
                                engine_state = WorkerState::Idle;
                                break;
                            }
                        }
                    }
                }
            } else {
                // No command channel — can't unpause, just stop
                info!("paused without command channel, stopping");
                break;
            }
        }

        if engine_state == WorkerState::Idle {
            break;
        }

        stats.round += 1;
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
            if stats.round == 1 || stats.round.is_multiple_of(10) {
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
            carry_state: carry_state.take(),
        };

        // Recompute batch size in case config changed
        let batch_size = if config.max_submissions_per_round > 0 {
            config.max_submissions_per_round
        } else {
            20
        };

        // ── Run search ──────────────────────────────────────
        let strategy_clone = strategy.clone();

        let raw_discoveries: Vec<RawDiscovery>;
        let mut result;

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

        // Save carry_state for next round
        carry_state = result.carry_state.take();

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
        let mut dash_discoveries_sent: usize = 0;
        const MAX_DASH_DISCOVERIES_PER_ROUND: usize = 20;

        // Cap post-search scoring to avoid spending more time scoring than searching.
        // With polish capped at 5/depth, we still get thousands of raw discoveries
        // from the beam search itself — most will be threshold-gated anyway.
        const MAX_SCORE_PER_ROUND: usize = 200;
        let scoring_limit = raw_discoveries.len().min(MAX_SCORE_PER_ROUND);

        for discovery in &raw_discoveries[..scoring_limit] {
            // Canonical form + aut_order
            let (canonical, aut_order) = canonical_form(&discovery.graph);
            let canonical_g6 = graph6::encode(&canonical);
            let cid = extremal_graph::compute_cid(&canonical);

            // Score locally
            let histogram = CliqueHistogram::compute(&discovery.graph, max_k);
            let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
            let gap = goodman::goodman_gap(config.n, red_tri, blue_tri);
            let score = GraphScore::new(histogram, gap, aut_order, cid);
            let score_bytes = score.to_score_bytes(max_k);

            // Send unique scored discoveries to dashboard (capped per round)
            if let Some(ref dash) = dashboard
                && !dash_sent_cids.contains(&cid)
                && dash_discoveries_sent < MAX_DASH_DISCOVERIES_PER_ROUND
            {
                dash_sent_cids.insert(cid);
                dash_discoveries_sent += 1;
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
        stats.total_discoveries += new_unique as u64;
        total_skipped_dup += round_skipped_dup;
        total_skipped_server += round_skipped_server;
        total_skipped_threshold += round_skipped_threshold;

        // Add to submit buffer and sort by score (best first)
        submit_buffer.extend(new_scored);
        submit_buffer.sort_by(|a, b| a.score.cmp(&b.score));

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

        stats.total_submitted += round_submitted;
        stats.total_admitted += round_admitted;

        let round_elapsed = round_start.elapsed();
        stats.last_round_ms = round_elapsed.as_millis() as u64;

        info!(
            round = stats.round,
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
            ms = stats.last_round_ms,
            stats.total_discoveries,
            stats.total_admitted,
            "round complete"
        );

        // Send round summary to dashboard
        if let Some(ref dash) = dashboard {
            dash.send(WorkerMessage::RoundComplete {
                round: stats.round,
                duration_ms: stats.last_round_ms,
                discoveries: new_unique as u64,
                submitted: round_submitted,
                admitted: round_admitted,
                buffered: submit_buffer.len(),
            });
        }

        // Update snapshot
        stats.submit_buffer_size = submit_buffer.len();
        stats.known_cids_count = known_cids.len();
        stats.server_cids_count = server_cids.len();
        if let Some(ref tx) = snapshot_tx {
            let _ = tx.send(build_snapshot(
                engine_state.clone(),
                &config,
                strategy.as_ref(),
                &worker_id,
                &key_id,
                &stats,
            ));
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
        // Trim dashboard dedup set (same cap as known_cids)
        if dash_sent_cids.len() > config.max_known_cids {
            let target = config.max_known_cids / 2;
            let drain: Vec<_> = dash_sent_cids
                .iter()
                .take(dash_sent_cids.len() - target)
                .copied()
                .collect();
            for cid in drain {
                dash_sent_cids.remove(&cid);
            }
        }

        if shutdown.has_changed().unwrap_or(false) && *shutdown.borrow() {
            info!("shutdown signal received after round");
            break;
        }
    }

    info!(
        rounds = stats.round,
        stats.total_discoveries,
        stats.total_submitted,
        stats.total_admitted,
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
                extremal_strategies::init::perturb(&mut matrix, noise_flips, rng);
            }
            return Some(matrix);
        }
    }

    let mut seed = extremal_strategies::init::paley_graph(n);
    if noise_flips > 0 {
        extremal_strategies::init::perturb(&mut seed, noise_flips, rng);
    }
    Some(seed)
}
