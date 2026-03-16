//! Live search visualization and control via embedded web server.
//!
//! When `--port` is set, an axum server streams search snapshots to a
//! browser over WebSocket at ~20fps, displays a local leaderboard of
//! discoveries, and provides a control panel for starting/stopping searches.

pub mod server;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use ramseynet_graph::{rgxf, AdjacencyMatrix};
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

/// A ranked entry in the local viz leaderboard.
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
    pub rank: usize,
    pub times_found: u64,
}

/// Top-N leaderboard that tracks the best discoveries (viz-local).
struct Leaderboard {
    entries: Vec<LeaderboardEntry>,
    cid_index: HashMap<String, usize>,
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

    fn submit(&mut self, entry: LeaderboardEntry) -> Option<LeaderboardEntry> {
        let cid = entry.cid.clone();

        if let Some(&idx) = self.cid_index.get(&cid) {
            self.entries[idx].times_found += 1;
            return Some(self.entries[idx].clone());
        }

        let pos = self
            .entries
            .binary_search_by(|e| e.score.cmp(&entry.score))
            .unwrap_or_else(|p| p);

        if pos >= self.capacity && self.entries.len() >= self.capacity {
            return None;
        }

        self.entries.insert(pos, entry);

        if self.entries.len() > self.capacity {
            let evicted = self.entries.pop().unwrap();
            self.cid_index.remove(&evicted.cid);
        }

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

/// Tagged message sent over the WebSocket to the browser.
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
    Strategies {
        strategies: Vec<ramseynet_worker_api::StrategyInfo>,
    },
}

/// Handle for pushing viz updates from the engine.
///
/// Thread-safe: the engine calls `update_snapshot` and `submit_discovery`
/// from various threads/tasks. The viz server subscribes to watch channels.
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

    /// Submit a scored discovery to the viz leaderboard.
    pub fn submit_discovery(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        is_record: bool,
        score: GraphScore,
    ) -> Option<LeaderboardEntry> {
        let entry = LeaderboardEntry {
            cid: score.cid.to_hex(),
            graph: rgxf::to_json(graph),
            n,
            strategy: strategy.to_string(),
            iteration,
            is_record,
            found_at_ms: self.start_time.elapsed().as_millis() as u64,
            score,
            rank: 0,
            times_found: 1,
        };

        let mut lb = self
            .leaderboard
            .lock()
            .expect("viz leaderboard lock poisoned");
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
