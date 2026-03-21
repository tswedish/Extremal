//! Protocol types for dashboard WebSocket communication.
//!
//! These types define the messages exchanged between:
//! - Workers → Dashboard relay server
//! - Dashboard relay server → Browser UI

use serde::{Deserialize, Serialize};

// ── Worker → Dashboard ──────────────────────────────────────

/// Messages sent from a worker to the dashboard relay server.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerMessage {
    /// Initial registration on connect.
    Register {
        key_id: String,
        worker_id: String,
        n: u32,
        strategy: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    /// Periodic progress update (~every 100 iterations).
    Progress {
        iteration: u64,
        max_iters: u64,
        violation_score: u32,
        current_graph6: String,
        discoveries_so_far: u64,
    },
    /// A valid graph was discovered.
    Discovery {
        graph6: String,
        cid: String,
        goodman_gap: f64,
        aut_order: f64,
        score_hex: String,
        histogram: Vec<(u32, u64, u64)>,
        iteration: u64,
    },
    /// A search round completed.
    RoundComplete {
        round: u64,
        duration_ms: u64,
        discoveries: u64,
        submitted: u64,
        admitted: u64,
        buffered: usize,
    },
}

// ── Dashboard → Worker (future) ─────────────────────────────

/// Commands sent from the dashboard to a worker.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum DashboardCommand {
    Pause,
    Resume,
    Stop,
    UpdateConfig { config: serde_json::Value },
}

// ── Dashboard → Browser UI ──────────────────────────────────

/// Events sent from the dashboard relay server to browser clients.
/// Worker events are wrapped with the worker_id for multiplexing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::enum_variant_names)]
pub enum UiEvent {
    /// A worker connected and registered.
    WorkerConnected {
        worker_id: String,
        key_id: String,
        n: u32,
        strategy: String,
        metadata: Option<serde_json::Value>,
    },
    /// A worker disconnected.
    WorkerDisconnected { worker_id: String },
    /// A worker event, tagged with worker_id.
    WorkerEvent {
        worker_id: String,
        event: WorkerMessage,
    },
}

// ── Browser → Dashboard (future) ────────────────────────────

/// Commands from the browser to the dashboard (forwarded to workers).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum UiCommand {
    /// Forward a command to a specific worker.
    WorkerCommand {
        worker_id: String,
        command: DashboardCommand,
    },
}
