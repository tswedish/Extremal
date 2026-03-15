//! Live search visualization via embedded web server.
//!
//! When `--port` is set, an axum server streams search snapshots
//! to a browser over WebSocket at ~20fps. Valid graphs are scored and
//! ranked in a top-N leaderboard (capacity 100 for local viz, display limit selectable in UI).

pub mod server;

use std::collections::HashMap;
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
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
    #[serde(rename = "status")]
    Status(ramseynet_worker_api::WorkerStatus),
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "strategies")]
    Strategies { strategies: Vec<ramseynet_worker_api::StrategyInfo> },
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
        // Use the CID embedded in the score (canonical CID from compute_score_canonical)
        // rather than recomputing from the graph. This ensures the viz leaderboard
        // deduplicates by canonical CID even if the passed graph isn't canonical.
        let entry = LeaderboardEntry {
            cid: score.cid.to_hex(),
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

    /// Get a snapshot of known canonical CIDs from prior rounds/submissions.
    /// Search strategies can use this to pre-seed deduplication sets.
    /// Default returns empty (no prior knowledge).
    fn known_cids(&self) -> std::collections::HashSet<GraphCid> {
        std::collections::HashSet::new()
    }

    /// Check if the search has been cancelled (e.g. Ctrl+C shutdown).
    /// Strategies should check this periodically (every ~100 iterations)
    /// and return their best result early when true.
    fn is_cancelled(&self) -> bool {
        false
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

/// Thread-safe set of canonical CIDs that have already been seen — either
/// from the server leaderboard, prior submissions, or earlier search rounds.
/// Shared across the worker lifetime so discoveries are never re-submitted.
#[derive(Clone, Default)]
pub struct KnownCids {
    inner: Arc<Mutex<std::collections::HashSet<GraphCid>>>,
}

impl KnownCids {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add CIDs from the server leaderboard (hex strings → GraphCid).
    pub fn add_from_hex(&self, hex_cids: &[String]) {
        let mut set = self.inner.lock().unwrap();
        for hex in hex_cids {
            if let Ok(cid) = GraphCid::from_hex(hex) {
                set.insert(cid);
            }
        }
    }

    /// Add a single CID (after submission or discovery).
    pub fn insert(&self, cid: GraphCid) {
        self.inner.lock().unwrap().insert(cid);
    }

    /// Add a single CID from hex string.
    pub fn insert_hex(&self, hex: &str) {
        if let Ok(cid) = GraphCid::from_hex(hex) {
            self.inner.lock().unwrap().insert(cid);
        }
    }

    /// Check if a CID is already known.
    pub fn contains(&self, cid: &GraphCid) -> bool {
        self.inner.lock().unwrap().contains(cid)
    }

    /// Clone the set of known CIDs (for seeding a tree search `seen` set).
    pub fn snapshot(&self) -> std::collections::HashSet<GraphCid> {
        self.inner.lock().unwrap().clone()
    }

    /// Number of known CIDs.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Thread-safe collector that keeps only the best `capacity` discoveries,
/// sorted by score (best first) with CID dedup. Acts as a mini-leaderboard
/// so the worker only submits competitive graphs to the server.
///
/// Optionally references a `KnownCids` set to skip graphs that have
/// already been submitted or seen in prior rounds.
#[derive(Clone)]
pub struct DiscoveryCollector {
    inner: Arc<Mutex<CollectorInner>>,
    known: Option<KnownCids>,
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
        Self::with_capacity(1000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CollectorInner {
                entries: Vec::new(),
                cid_set: std::collections::HashSet::new(),
                capacity,
            })),
            known: None,
        }
    }

    /// Create a collector that cross-references a shared `KnownCids` set.
    /// Graphs already in `known` are silently rejected on push.
    pub fn with_known(capacity: usize, known: KnownCids) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CollectorInner {
                entries: Vec::new(),
                cid_set: std::collections::HashSet::new(),
                capacity,
            })),
            known: Some(known),
        }
    }

    /// Insert a discovery, maintaining sorted order and bounded capacity.
    /// Duplicates (by CID, including cross-round known CIDs) are silently ignored.
    pub fn push(&self, discovery: Discovery) {
        // Check global known CIDs first (lock-free relative to inner)
        if let Some(ref known) = self.known {
            if known.contains(&discovery.cid) {
                return;
            }
        }

        let mut inner = self.inner.lock().unwrap();

        // Local CID dedup within this collector
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

        // NOTE: We intentionally do NOT add to `known` here. The known set
        // tracks graphs that have been *submitted to the server*. Adding on
        // push would cause submit_discoveries() to skip every graph (since
        // it checks known.contains() before POSTing). The known set is
        // populated after successful server submission instead.

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
///
/// Valid graphs are canonicalized once and the canonical form is used for
/// both the collector (server submission) and the viz leaderboard, ensuring
/// consistent CID deduplication across both surfaces.
pub struct CollectorObserver {
    pub collector: DiscoveryCollector,
    /// VizObserver for progress snapshots (iteration updates).
    viz: Option<VizObserver>,
    /// Direct handle to the viz leaderboard — used to submit canonical
    /// graphs directly, bypassing VizObserver::on_valid_found (which
    /// would receive the raw graph and compute a non-canonical CID).
    viz_handle: Option<Arc<VizHandle>>,
    /// Known CIDs for seeding tree search `seen` sets.
    known: Option<KnownCids>,
    /// Cancellation flag — set by the worker on Ctrl+C / shutdown.
    cancelled: Arc<AtomicBool>,
}

impl CollectorObserver {
    pub fn new(collector: DiscoveryCollector, viz: Option<VizObserver>) -> Self {
        let known = collector.known.clone();
        let viz_handle = viz.as_ref().map(|v| Arc::clone(&v.handle));
        Self {
            collector,
            viz,
            viz_handle,
            known,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create with a shared cancellation flag (so the worker can set it).
    pub fn with_cancel(
        collector: DiscoveryCollector,
        viz: Option<VizObserver>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        let known = collector.known.clone();
        let viz_handle = viz.as_ref().map(|v| Arc::clone(&v.handle));
        Self { collector, viz, viz_handle, known, cancelled }
    }

    /// Signal cancellation — search strategies will bail out early.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Get a snapshot of all known canonical CIDs for seeding a search's
    /// dedup set. Tree search uses this to pre-populate its `seen` HashSet
    /// so it never wastes iterations on graphs already discovered.
    pub fn known_cid_snapshot(&self) -> std::collections::HashSet<GraphCid> {
        match &self.known {
            Some(known) => known.snapshot(),
            None => std::collections::HashSet::new(),
        }
    }
}

impl SearchObserver for CollectorObserver {
    fn on_progress(&self, info: &ProgressInfo) {
        if let Some(ref viz) = self.viz {
            viz.on_progress(info);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn known_cids(&self) -> std::collections::HashSet<GraphCid> {
        self.known_cid_snapshot()
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
        // Canonicalize once — used for both the collector (server submission)
        // and the viz leaderboard, ensuring consistent CID dedup.
        let score_result = ramseynet_verifier::scoring::compute_score_canonical(graph);
        let canonical_cid = compute_cid(&score_result.canonical_graph);
        self.collector.push(Discovery {
            graph: score_result.canonical_graph.clone(),
            score: score_result.score.clone(),
            cid: canonical_cid,
        });
        // Submit the CANONICAL graph to the viz leaderboard directly,
        // bypassing VizObserver::on_valid_found which would use the raw graph.
        if let Some(ref vh) = self.viz_handle {
            vh.submit_discovery(
                &score_result.canonical_graph,
                n, strategy, iteration,
                false, score_result.score,
            );
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

    // on_valid_found: uses the default no-op. CollectorObserver handles
    // valid graph submissions directly to VizHandle with canonical form.
}
