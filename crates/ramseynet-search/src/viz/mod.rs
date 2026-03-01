//! Live search visualization via embedded web server.
//!
//! When `--viz-port` is set, an axum server streams search snapshots
//! to a browser over WebSocket at ~20fps.

pub mod server;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use ramseynet_graph::{rgxf, AdjacencyMatrix};
use ramseynet_verifier::clique::find_clique_witness;
use serde::Serialize;
use tokio::sync::{broadcast, watch};

/// Rarity tier for a valid Ramsey graph based on clique/independence proximity.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RarityTier {
    Common,
    Uncommon,
    Rare,
    Legendary,
}

/// Compute the rarity tier of a valid R(k, ell) graph.
///
/// Returns `(tier, omega, alpha)` where omega is the clique number and
/// alpha is the independence number.
pub fn compute_rarity(
    graph: &AdjacencyMatrix,
    k: u32,
    ell: u32,
    is_record: bool,
) -> (RarityTier, u32, u32) {
    // Find omega: largest clique in G
    let mut omega = 1;
    for size in (2..k).rev() {
        if find_clique_witness(graph, size).is_some() {
            omega = size;
            break;
        }
    }

    // Find alpha: largest independent set = largest clique in complement
    let comp = graph.complement();
    let mut alpha = 1;
    for size in (2..ell).rev() {
        if find_clique_witness(&comp, size).is_some() {
            alpha = size;
            break;
        }
    }

    let tier = if is_record {
        RarityTier::Legendary
    } else if omega == k - 1 && alpha == ell - 1 {
        RarityTier::Rare
    } else if omega == k - 1 || alpha == ell - 1 {
        RarityTier::Uncommon
    } else {
        RarityTier::Common
    };

    (tier, omega, alpha)
}

/// A snapshot of the current search state, sent to the browser at ~20fps.
#[derive(Clone, Debug, Serialize)]
pub struct SearchSnapshot {
    pub graph: ramseynet_graph::rgxf::RgxfJson,
    pub n: u32,
    pub k: u32,
    pub ell: u32,
    pub strategy: String,
    pub iteration: u64,
    pub max_iters: u64,
    pub valid: bool,
    pub edges: u32,
    pub violation_score: u32,
    pub elapsed_ms: u64,
    pub throughput: f64,
}

/// A valid graph that was pinned (discovered) during search.
#[derive(Clone, Debug, Serialize)]
pub struct PinnedGraph {
    pub graph: ramseynet_graph::rgxf::RgxfJson,
    pub n: u32,
    pub strategy: String,
    pub iteration: u64,
    pub is_record: bool,
    pub found_at_ms: u64,
    pub rarity: RarityTier,
    pub omega: u32,
    pub alpha: u32,
}

/// Tagged message sent over the WebSocket.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum VizMessage {
    #[serde(rename = "hello")]
    Hello { version: String },
    #[serde(rename = "snapshot")]
    Snapshot(SearchSnapshot),
    #[serde(rename = "pinned")]
    Pinned(PinnedGraph),
}

/// Handle that search threads use to push updates to the viz server.
pub struct VizHandle {
    snapshot_tx: watch::Sender<Option<SearchSnapshot>>,
    pinned_tx: broadcast::Sender<PinnedGraph>,
    start_time: Instant,
}

impl Default for VizHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl VizHandle {
    pub fn new() -> Self {
        let (snapshot_tx, _) = watch::channel(None);
        let (pinned_tx, _) = broadcast::channel(64);
        Self {
            snapshot_tx,
            pinned_tx,
            start_time: Instant::now(),
        }
    }

    pub fn update_snapshot(&self, snapshot: SearchSnapshot) {
        let _ = self.snapshot_tx.send(Some(snapshot));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pin_graph(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        is_record: bool,
        rarity: RarityTier,
        omega: u32,
        alpha: u32,
    ) {
        let pinned = PinnedGraph {
            graph: rgxf::to_json(graph),
            n,
            strategy: strategy.to_string(),
            iteration,
            is_record,
            found_at_ms: self.start_time.elapsed().as_millis() as u64,
            rarity,
            omega,
            alpha,
        };
        let _ = self.pinned_tx.send(pinned);
    }

    pub fn subscribe_snapshot(&self) -> watch::Receiver<Option<SearchSnapshot>> {
        self.snapshot_tx.subscribe()
    }

    pub fn subscribe_pinned(&self) -> broadcast::Receiver<PinnedGraph> {
        self.pinned_tx.subscribe()
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

/// Trait for observing search progress. Implementations must be Send + Sync
/// so they can be passed into `spawn_blocking`.
#[allow(clippy::too_many_arguments)]
pub trait SearchObserver: Send + Sync {
    fn on_progress(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        k: u32,
        ell: u32,
        strategy: &str,
        iteration: u64,
        max_iters: u64,
        valid: bool,
        violation_score: u32,
    );
}

/// No-op observer — zero overhead when viz is disabled.
pub struct NoOpObserver;

impl SearchObserver for NoOpObserver {
    #[inline]
    fn on_progress(
        &self,
        _graph: &AdjacencyMatrix,
        _n: u32,
        _k: u32,
        _ell: u32,
        _strategy: &str,
        _iteration: u64,
        _max_iters: u64,
        _valid: bool,
        _violation_score: u32,
    ) {
    }
}

/// Observer that throttles updates to ~20fps and sends them to VizHandle.
pub struct VizObserver {
    handle: Arc<VizHandle>,
    last_update: Mutex<Instant>,
    /// EMA state: (last_iteration, last_instant, smoothed_throughput)
    ema: Mutex<(u64, Instant, f64)>,
}

/// EMA smoothing factor — 0.3 reacts quickly (settles in ~3 ticks / ~150ms).
const EMA_ALPHA: f64 = 0.3;

impl VizObserver {
    pub fn new(handle: Arc<VizHandle>) -> Self {
        let now = Instant::now();
        Self {
            handle,
            last_update: Mutex::new(now),
            ema: Mutex::new((0, now, 0.0)),
        }
    }
}

impl SearchObserver for VizObserver {
    fn on_progress(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        k: u32,
        ell: u32,
        strategy: &str,
        iteration: u64,
        max_iters: u64,
        valid: bool,
        violation_score: u32,
    ) {
        let now = Instant::now();
        let mut last = self.last_update.lock().unwrap();
        if now.duration_since(*last).as_millis() < 50 {
            return;
        }
        *last = now;

        let elapsed_ms = self.handle.elapsed_ms();

        // Compute throughput as EMA of instantaneous rate between ticks
        let throughput = {
            let mut ema = self.ema.lock().unwrap();
            let dt_secs = now.duration_since(ema.1).as_secs_f64();
            let d_iters = iteration.saturating_sub(ema.0);
            let instant_rate = if dt_secs > 0.0 {
                d_iters as f64 / dt_secs
            } else {
                ema.2
            };
            // Reset EMA on iteration drops (new search round) or first tick
            let smoothed = if iteration < ema.0 || ema.2 == 0.0 {
                instant_rate
            } else {
                EMA_ALPHA * instant_rate + (1.0 - EMA_ALPHA) * ema.2
            };
            *ema = (iteration, now, smoothed);
            smoothed
        };

        let snapshot = SearchSnapshot {
            graph: rgxf::to_json(graph),
            n,
            k,
            ell,
            strategy: strategy.to_string(),
            iteration,
            max_iters,
            valid,
            edges: graph.num_edges() as u32,
            violation_score,
            elapsed_ms,
            throughput,
        };
        self.handle.update_snapshot(snapshot);
    }
}
