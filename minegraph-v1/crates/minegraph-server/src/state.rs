//! Application state shared across all handlers.

use minegraph_identity::Identity;
use minegraph_store::Store;
use std::sync::Arc;
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
}
