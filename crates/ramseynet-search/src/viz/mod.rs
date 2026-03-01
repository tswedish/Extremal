//! Live search visualization via embedded web server.
//!
//! When `--viz-port` is set, an axum server streams search snapshots
//! to a browser over WebSocket at ~20fps. Valid graphs are scored and
//! ranked in a top-N leaderboard (capacity 100, display limit selectable in UI).

pub mod server;

use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ramseynet_graph::{compute_cid, rgxf, AdjacencyMatrix};
use ramseynet_types::GraphCid;
use ramseynet_verifier::scoring::GraphScore;
use serde::Serialize;
use tokio::sync::watch;

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
    pub k_cliques: Option<u64>,
    pub ell_indsets: Option<u64>,
    pub elapsed_ms: u64,
    pub throughput: f64,
}

/// A ranked entry in the leaderboard.
#[derive(Clone, Debug, Serialize)]
pub struct LeaderboardEntry {
    pub cid: String,
    pub graph: ramseynet_graph::rgxf::RgxfJson,
    pub n: u32,
    pub strategy: String,
    pub iteration: u64,
    pub is_record: bool,
    pub found_at_ms: u64,
    pub score: GraphScore,
    pub rank: usize,       // 1-based
    pub times_found: u64,  // CID dedup counter
}

/// Top-N leaderboard that tracks the best discoveries.
struct Leaderboard {
    entries: Vec<LeaderboardEntry>,     // sorted best-first
    cid_index: HashMap<String, usize>,  // CID → index in entries
    capacity: usize,
}

impl Leaderboard {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            cid_index: HashMap::new(),
            capacity,
        }
    }

    /// Try to submit a discovery. Returns the entry if it was accepted (new or dedup).
    fn submit(&mut self, entry: LeaderboardEntry) -> Option<LeaderboardEntry> {
        let cid = entry.cid.clone();

        // CID dedup: increment count if already on the board
        if let Some(&idx) = self.cid_index.get(&cid) {
            self.entries[idx].times_found += 1;
            return Some(self.entries[idx].clone());
        }

        // Find insertion position (sorted by score ascending = best first)
        let pos = self
            .entries
            .binary_search_by(|e| e.score.cmp(&entry.score))
            .unwrap_or_else(|p| p);

        // Reject if board is full and this would go past capacity
        if pos >= self.capacity && self.entries.len() >= self.capacity {
            return None;
        }

        // Insert
        self.entries.insert(pos, entry);

        // Evict worst if over capacity
        if self.entries.len() > self.capacity {
            let evicted = self.entries.pop().unwrap();
            self.cid_index.remove(&evicted.cid);
        }

        // Rebuild index and ranks
        self.cid_index.clear();
        for (i, e) in self.entries.iter_mut().enumerate() {
            e.rank = i + 1;
            self.cid_index.insert(e.cid.clone(), i);
        }

        let accepted_idx = self.cid_index[&cid];
        Some(self.entries[accepted_idx].clone())
    }

    fn entries(&self) -> Vec<LeaderboardEntry> {
        self.entries.clone()
    }
}

/// Tagged message sent over the WebSocket.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum VizMessage {
    #[serde(rename = "hello")]
    Hello { version: String },
    #[serde(rename = "snapshot")]
    Snapshot(SearchSnapshot),
    #[serde(rename = "leaderboard")]
    Leaderboard { entries: Vec<LeaderboardEntry> },
}

/// Handle that search threads use to push updates to the viz server.
pub struct VizHandle {
    snapshot_tx: watch::Sender<Option<SearchSnapshot>>,
    leaderboard: Mutex<Leaderboard>,
    leaderboard_tx: watch::Sender<Vec<LeaderboardEntry>>,
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
        let (leaderboard_tx, _) = watch::channel(Vec::new());
        Self {
            snapshot_tx,
            leaderboard: Mutex::new(Leaderboard::new(100)),
            leaderboard_tx,
            start_time: Instant::now(),
        }
    }

    pub fn update_snapshot(&self, snapshot: SearchSnapshot) {
        let _ = self.snapshot_tx.send(Some(snapshot));
    }

    /// Submit a discovery with a pre-computed score to the leaderboard.
    /// Checks admission and broadcasts if accepted.
    ///
    /// **Important:** Call this from a blocking context (or after `spawn_blocking`)
    /// since the caller is responsible for computing the score via `compute_score`.
    ///
    /// Returns the LeaderboardEntry if the graph was accepted (or deduped).
    pub fn submit_discovery(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        is_record: bool,
        score: GraphScore,
    ) -> Option<LeaderboardEntry> {
        let cid = compute_cid(graph);
        let entry = LeaderboardEntry {
            cid: cid.to_hex(),
            graph: rgxf::to_json(graph),
            n,
            strategy: strategy.to_string(),
            iteration,
            is_record,
            found_at_ms: self.start_time.elapsed().as_millis() as u64,
            score,
            rank: 0, // will be set by leaderboard
            times_found: 1,
        };

        let mut lb = self.leaderboard.lock().unwrap();
        let result = lb.submit(entry);

        if result.is_some() {
            let _ = self.leaderboard_tx.send(lb.entries());
        }

        result
    }

    pub fn subscribe_snapshot(&self) -> watch::Receiver<Option<SearchSnapshot>> {
        self.snapshot_tx.subscribe()
    }

    pub fn subscribe_leaderboard(&self) -> watch::Receiver<Vec<LeaderboardEntry>> {
        self.leaderboard_tx.subscribe()
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

/// Bundled progress info passed to observers.
pub struct ProgressInfo<'a> {
    pub graph: &'a AdjacencyMatrix,
    pub n: u32,
    pub k: u32,
    pub ell: u32,
    pub strategy: &'a str,
    pub iteration: u64,
    pub max_iters: u64,
    pub valid: bool,
    pub violation_score: u32,
    pub k_cliques: Option<u64>,
    pub ell_indsets: Option<u64>,
}

/// Trait for observing search progress. Implementations must be Send + Sync
/// so they can be passed into `spawn_blocking`.
pub trait SearchObserver: Send + Sync {
    fn on_progress(&self, info: &ProgressInfo);

    /// Called when a valid graph is found mid-search (e.g. during tree/beam search).
    /// Default is a no-op. VizObserver submits immediately to the leaderboard.
    fn on_valid_found(
        &self,
        _graph: &AdjacencyMatrix,
        _n: u32,
        _k: u32,
        _ell: u32,
        _strategy: &str,
        _iteration: u64,
    ) {
    }
}

/// No-op observer — zero overhead when viz is disabled.
pub struct NoOpObserver;

impl SearchObserver for NoOpObserver {
    #[inline]
    fn on_progress(&self, _info: &ProgressInfo) {}
}

/// A valid graph discovered mid-search, ready for server submission.
pub struct Discovery {
    pub graph: AdjacencyMatrix,
    pub score: GraphScore,
    pub cid: GraphCid,
}

/// Thread-safe collector that keeps only the best `capacity` discoveries,
/// sorted by score (best first) with CID dedup. Acts as a mini-leaderboard
/// so the worker only submits competitive graphs to the server.
#[derive(Clone)]
pub struct DiscoveryCollector {
    inner: Arc<Mutex<CollectorInner>>,
}

struct CollectorInner {
    entries: Vec<Discovery>,
    cid_set: std::collections::HashSet<GraphCid>,
    capacity: usize,
}

impl Default for DiscoveryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoveryCollector {
    pub fn new() -> Self {
        Self::with_capacity(100)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CollectorInner {
                entries: Vec::new(),
                cid_set: std::collections::HashSet::new(),
                capacity,
            })),
        }
    }

    /// Insert a discovery, maintaining sorted order and bounded capacity.
    /// Duplicates (by CID) are silently ignored.
    pub fn push(&self, discovery: Discovery) {
        let mut inner = self.inner.lock().unwrap();

        // CID dedup
        if inner.cid_set.contains(&discovery.cid) {
            return;
        }

        // Find insertion position (sorted by score ascending = best first)
        let pos = inner
            .entries
            .binary_search_by(|e| e.score.cmp(&discovery.score))
            .unwrap_or_else(|p| p);

        // Reject if full and this would go past capacity
        if pos >= inner.capacity && inner.entries.len() >= inner.capacity {
            return;
        }

        inner.cid_set.insert(discovery.cid.clone());
        inner.entries.insert(pos, discovery);

        // Evict worst if over capacity
        if inner.entries.len() > inner.capacity {
            let evicted = inner.entries.pop().unwrap();
            inner.cid_set.remove(&evicted.cid);
        }
    }

    /// Drain all collected discoveries (best first), leaving the collector empty.
    pub fn drain(&self) -> Vec<Discovery> {
        let mut inner = self.inner.lock().unwrap();
        inner.cid_set.clear();
        mem::take(&mut inner.entries)
    }
}

/// Observer that collects all valid discoveries and optionally forwards to a
/// `VizObserver` for live dashboard display. Replaces the old pattern of
/// choosing between `VizObserver` and `NoOpObserver`.
pub struct CollectorObserver {
    pub collector: DiscoveryCollector,
    viz: Option<VizObserver>,
}

impl CollectorObserver {
    pub fn new(collector: DiscoveryCollector, viz: Option<VizObserver>) -> Self {
        Self { collector, viz }
    }
}

impl SearchObserver for CollectorObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        if let Some(ref viz) = self.viz {
            viz.on_progress(info);
        }
    }

    fn on_valid_found(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        k: u32,
        ell: u32,
        strategy: &str,
        iteration: u64,
    ) {
        let cid = compute_cid(graph);
        let score = ramseynet_verifier::scoring::compute_score(graph, &cid);
        self.collector.push(Discovery {
            graph: graph.clone(),
            score: score.clone(),
            cid: cid.clone(),
        });
        if let Some(ref viz) = self.viz {
            viz.on_valid_found(graph, n, k, ell, strategy, iteration);
        }
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
    fn on_progress(&self, info: &ProgressInfo) {
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
            let d_iters = info.iteration.saturating_sub(ema.0);
            let instant_rate = if dt_secs > 0.0 {
                d_iters as f64 / dt_secs
            } else {
                ema.2
            };
            // Reset EMA on iteration drops (new search round) or first tick
            let smoothed = if info.iteration < ema.0 || ema.2 == 0.0 {
                instant_rate
            } else {
                EMA_ALPHA * instant_rate + (1.0 - EMA_ALPHA) * ema.2
            };
            *ema = (info.iteration, now, smoothed);
            smoothed
        };

        let snapshot = SearchSnapshot {
            graph: rgxf::to_json(info.graph),
            n: info.n,
            k: info.k,
            ell: info.ell,
            strategy: info.strategy.to_string(),
            iteration: info.iteration,
            max_iters: info.max_iters,
            valid: info.valid,
            edges: info.graph.num_edges() as u32,
            violation_score: info.violation_score,
            k_cliques: info.k_cliques,
            ell_indsets: info.ell_indsets,
            elapsed_ms,
            throughput,
        };
        self.handle.update_snapshot(snapshot);
    }

    fn on_valid_found(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        _k: u32,
        _ell: u32,
        strategy: &str,
        iteration: u64,
    ) {
        let cid = compute_cid(graph);
        let score = ramseynet_verifier::scoring::compute_score(graph, &cid);
        self.handle
            .submit_discovery(graph, n, strategy, iteration, false, score);
    }
}
