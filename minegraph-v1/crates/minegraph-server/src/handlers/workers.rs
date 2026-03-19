use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::{AppState, ServerEvent, WorkerHeartbeat};

/// POST /api/workers/heartbeat — worker sends stats.
pub async fn worker_heartbeat(
    State(state): State<AppState>,
    Json(heartbeat): Json<WorkerHeartbeat>,
) -> Result<Json<Value>, ApiError> {
    let worker_id = heartbeat.worker_id.clone();

    // Broadcast as SSE event
    let _ = state.events_tx.send(ServerEvent::WorkerHeartbeat {
        worker_id: heartbeat.worker_id.clone(),
        stats: heartbeat.stats.clone(),
    });

    // Store in memory
    state
        .workers
        .lock()
        .unwrap()
        .insert(heartbeat.worker_id.clone(), heartbeat);

    Ok(Json(json!({ "ok": true, "worker_id": worker_id })))
}

/// GET /api/workers — list all known workers with stats.
pub async fn list_workers(State(state): State<AppState>) -> Json<Value> {
    let workers = state.workers.lock().unwrap();

    // Filter out stale workers (no heartbeat in 60 seconds)
    let now = chrono::Utc::now();
    let active: Vec<&WorkerHeartbeat> = workers
        .values()
        .filter(|w| (now - w.last_seen).num_seconds() < 60)
        .collect();

    let result: Vec<Value> = active
        .iter()
        .map(|w| {
            json!({
                "worker_id": w.worker_id,
                "key_id": w.key_id,
                "strategy": w.strategy,
                "n": w.n,
                "stats": w.stats,
                "metadata": w.metadata,
                "last_seen": w.last_seen.to_rfc3339(),
                "stale": false,
            })
        })
        .collect();

    // Also include stale workers
    let stale: Vec<Value> = workers
        .values()
        .filter(|w| (now - w.last_seen).num_seconds() >= 60)
        .map(|w| {
            json!({
                "worker_id": w.worker_id,
                "key_id": w.key_id,
                "strategy": w.strategy,
                "n": w.n,
                "stats": w.stats,
                "metadata": w.metadata,
                "last_seen": w.last_seen.to_rfc3339(),
                "stale": true,
            })
        })
        .collect();

    let mut all = result;
    all.extend(stale);

    Json(json!({
        "workers": all,
        "active_count": all.iter().filter(|w| !w["stale"].as_bool().unwrap_or(true)).count(),
    }))
}
