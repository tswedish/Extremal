//! Shared dashboard server state.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};

use crate::protocol::UiEvent;

/// Information about a connected worker.
#[derive(Clone, Debug)]
pub struct WorkerInfo {
    pub worker_id: String,
    pub key_id: String,
    pub n: u32,
    pub strategy: String,
    pub metadata: Option<serde_json::Value>,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Whether the worker's Ed25519 signature was verified.
    pub verified: bool,
    /// Worker's HTTP API address (e.g. "http://0.0.0.0:4001").
    pub api_addr: Option<String>,
}

/// Shared state for the dashboard relay server.
#[derive(Clone)]
pub struct DashboardState {
    /// Broadcast channel for UI events (relayed to all browser clients).
    pub ui_tx: broadcast::Sender<UiEvent>,
    /// Currently connected workers.
    pub workers: Arc<Mutex<HashMap<String, WorkerInfo>>>,
    /// Allow-list of key_ids. If empty, all keys are accepted.
    pub allowed_keys: Arc<Mutex<HashSet<String>>>,
    /// Maximum concurrent worker connections.
    pub max_workers: usize,
}

impl DashboardState {
    pub fn new(max_workers: usize, allowed_keys: HashSet<String>) -> Self {
        let (ui_tx, _) = broadcast::channel(256);
        Self {
            ui_tx,
            workers: Arc::new(Mutex::new(HashMap::new())),
            allowed_keys: Arc::new(Mutex::new(allowed_keys)),
            max_workers,
        }
    }

    /// Register a worker. Returns false if at capacity or key not allowed.
    ///
    /// When allow-list is active, requires both a verified signature AND
    /// key_id in the allow-list. When allow-list is empty (default), accepts all.
    pub async fn register_worker(&self, info: WorkerInfo) -> bool {
        let keys = self.allowed_keys.lock().await;
        if !keys.is_empty() && (!info.verified || !keys.contains(&info.key_id)) {
            return false;
        }
        drop(keys);

        let mut workers = self.workers.lock().await;
        if workers.len() >= self.max_workers && !workers.contains_key(&info.worker_id) {
            return false;
        }
        workers.insert(info.worker_id.clone(), info);
        true
    }

    /// Remove a worker on disconnect.
    pub async fn unregister_worker(&self, worker_id: &str) {
        self.workers.lock().await.remove(worker_id);
    }

    /// Get a snapshot of all connected workers.
    pub async fn list_workers(&self) -> Vec<WorkerInfo> {
        self.workers.lock().await.values().cloned().collect()
    }
}
