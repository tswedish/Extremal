//! Live search visualization via embedded web server.
//!
//! When `--viz-port` is set, an axum server streams search snapshots
//! to a browser over WebSocket at ~20fps.

pub mod server;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use ramseynet_graph::{rgxf, AdjacencyMatrix};
use serde::Serialize;
use tokio::sync::{broadcast, watch};

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

    pub fn pin_graph(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        is_record: bool,
    ) {
        let pinned = PinnedGraph {
            graph: rgxf::to_json(graph),
            n,
            strategy: strategy.to_string(),
            iteration,
            is_record,
            found_at_ms: self.start_time.elapsed().as_millis() as u64,
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
}

impl VizObserver {
    pub fn new(handle: Arc<VizHandle>) -> Self {
        Self {
            handle,
            last_update: Mutex::new(Instant::now()),
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
            elapsed_ms: self.handle.elapsed_ms(),
        };
        self.handle.update_snapshot(snapshot);
    }
}
