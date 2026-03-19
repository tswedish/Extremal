//! Application state shared across all handlers.

use minegraph_identity::Identity;
use minegraph_store::Store;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Server's own signing identity for receipts.
    pub server_identity: Arc<Identity>,
    /// Leaderboard capacity (max entries per n).
    pub leaderboard_capacity: i32,
    /// Maximum k for histogram scoring.
    pub max_k: u32,
    /// Broadcast channel for SSE events.
    pub events_tx: broadcast::Sender<ServerEvent>,
    /// In-memory worker heartbeat tracker.
    pub workers: Arc<Mutex<HashMap<String, WorkerHeartbeat>>>,
}

/// Events broadcast to SSE subscribers.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    /// A new graph was admitted to the leaderboard.
    #[serde(rename = "admission")]
    Admission {
        n: i32,
        cid: String,
        rank: i32,
        key_id: String,
    },
    /// A graph was submitted but not admitted.
    #[serde(rename = "submission")]
    Submission { n: i32, cid: String, key_id: String },
    /// A worker sent a heartbeat (for dashboard).
    #[serde(rename = "worker_heartbeat")]
    WorkerHeartbeat {
        worker_id: String,
        stats: WorkerStats,
    },
}

/// Worker heartbeat data stored in server memory.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkerHeartbeat {
    pub worker_id: String,
    pub key_id: String,
    pub strategy: String,
    pub n: u32,
    pub stats: WorkerStats,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

/// Runtime stats from a worker.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkerStats {
    pub round: u64,
    pub total_discoveries: u64,
    pub total_submitted: u64,
    pub total_admitted: u64,
    pub buffered: usize,
    pub last_round_ms: u64,
    pub new_unique_last_round: u64,
    pub uptime_secs: u64,
    /// Current best graph6 for visualization (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_graph6: Option<String>,
    /// Violation score of current best (0 = valid).
    #[serde(default)]
    pub violation_score: u32,
    /// Goodman gap of current best.
    #[serde(default)]
    pub goodman_gap: Option<f64>,
    /// |Aut(G)| of current best.
    #[serde(default)]
    pub aut_order: Option<f64>,
}
