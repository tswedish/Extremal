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
        let (ui_tx, _) = broadcast::channel(1024);
        Self {
            ui_tx,
            workers: Arc::new(Mutex::new(HashMap::new())),
            allowed_keys: Arc::new(Mutex::new(allowed_keys)),
            max_workers,
        }
    }

    /// Check if a key_id is allowed (empty allow-list = open access).
    pub async fn is_key_allowed(&self, key_id: &str) -> bool {
        let keys = self.allowed_keys.lock().await;
        keys.is_empty() || keys.contains(key_id)
    }

    /// Register a worker. Returns false if at capacity or key not allowed.
    pub async fn register_worker(&self, info: WorkerInfo) -> bool {
        if !self.is_key_allowed(&info.key_id).await {
            return false;
        }
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
