//! Dashboard relay server: HTTP + WebSocket endpoints.

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::protocol::{UiEvent, WorkerMessage};
use crate::state::{DashboardState, WorkerInfo};

// ── Worker WebSocket ────────────────────────────────────────

/// GET /ws/worker — WebSocket upgrade for workers.
pub async fn ws_worker(
    ws: WebSocketUpgrade,
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_worker(socket, state))
}

async fn handle_worker(mut socket: WebSocket, state: DashboardState) {
    // Wait for the Register message
    let register_msg = match wait_for_register(&mut socket).await {
        Some(msg) => msg,
        None => {
            warn!("worker connection closed before registration");
            return;
        }
    };

    let (key_id, worker_id, n, strategy, metadata) = match register_msg {
        WorkerMessage::Register {
            key_id,
            worker_id,
            n,
            strategy,
            metadata,
        } => (key_id, worker_id, n, strategy, metadata),
        _ => {
            warn!("first message from worker was not Register");
            let _ = socket
                .send(Message::Text(
                    json!({"error": "first message must be Register"})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    // Check auth
    let info = WorkerInfo {
        worker_id: worker_id.clone(),
        key_id: key_id.clone(),
        n,
        strategy: strategy.clone(),
        metadata: metadata.clone(),
        connected_at: chrono::Utc::now(),
    };

    if !state.register_worker(info).await {
        warn!(worker_id, key_id, "worker registration rejected");
        let _ = socket
            .send(Message::Text(
                json!({"error": "registration rejected (key not allowed or at capacity)"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    }

    info!(worker_id, key_id, n, strategy, "worker connected");

    // Broadcast connection event to UI
    let _ = state.ui_tx.send(UiEvent::WorkerConnected {
        worker_id: worker_id.clone(),
        key_id,
        n,
        strategy,
        metadata,
    });

    // Send ack to worker
    let _ = socket
        .send(Message::Text(
            json!({"ok": true, "worker_id": &worker_id})
                .to_string()
                .into(),
        ))
        .await;

    // Relay worker messages to UI subscribers
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => match serde_json::from_str::<WorkerMessage>(&text) {
                Ok(msg) => {
                    let _ = state.ui_tx.send(UiEvent::WorkerEvent {
                        worker_id: worker_id.clone(),
                        event: msg,
                    });
                }
                Err(e) => {
                    debug!(worker_id, "invalid message from worker: {e}");
                }
            },
            Some(Ok(Message::Close(_))) | None => break,
            Some(Ok(Message::Ping(data))) => {
                let _ = socket.send(Message::Pong(data)).await;
            }
            Some(Ok(_)) => {} // ignore binary, pong
            Some(Err(e)) => {
                debug!(worker_id, "worker ws error: {e}");
                break;
            }
        }
    }

    // Cleanup
    state.unregister_worker(&worker_id).await;
    let _ = state.ui_tx.send(UiEvent::WorkerDisconnected {
        worker_id: worker_id.clone(),
    });
    info!(worker_id, "worker disconnected");
}

async fn wait_for_register(socket: &mut WebSocket) -> Option<WorkerMessage> {
    // Give the worker 10 seconds to register
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), socket.recv()).await;

    match timeout {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text).ok(),
        _ => None,
    }
}

// ── UI WebSocket ────────────────────────────────────────────

/// GET /ws/ui — WebSocket upgrade for browser UI clients.
pub async fn ws_ui(ws: WebSocketUpgrade, State(state): State<DashboardState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ui(socket, state))
}

async fn handle_ui(mut socket: WebSocket, state: DashboardState) {
    info!("UI client connected");

    // Send current worker list as initial state
    let workers = state.list_workers().await;
    for w in workers {
        let event = UiEvent::WorkerConnected {
            worker_id: w.worker_id,
            key_id: w.key_id,
            n: w.n,
            strategy: w.strategy,
            metadata: w.metadata,
        };
        if let Ok(json) = serde_json::to_string(&event)
            && socket.send(Message::Text(json.into())).await.is_err()
        {
            return;
        }
    }

    // Subscribe to UI events and relay to browser
    let mut rx: broadcast::Receiver<UiEvent> = state.ui_tx.subscribe();

    loop {
        tokio::select! {
            // Relay events to browser
            event = rx.recv() => {
                match event {
                    Ok(ui_event) => {
                        if let Ok(json) = serde_json::to_string(&ui_event)
                            && socket.send(Message::Text(json.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("UI client lagged, dropped {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle messages from browser (future: commands)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Ok(_)) => {} // ignore for now
                    Some(Err(_)) => break,
                }
            }
        }
    }

    info!("UI client disconnected");
}

// ── REST endpoints ──────────────────────────────────────────

/// GET /api/workers — list connected workers.
pub async fn list_workers(State(state): State<DashboardState>) -> Json<serde_json::Value> {
    let workers = state.list_workers().await;
    let result: Vec<serde_json::Value> = workers
        .iter()
        .map(|w| {
            json!({
                "worker_id": w.worker_id,
                "key_id": w.key_id,
                "n": w.n,
                "strategy": w.strategy,
                "metadata": w.metadata,
                "connected_at": w.connected_at.to_rfc3339(),
            })
        })
        .collect();

    Json(json!({
        "workers": result,
        "count": result.len(),
    }))
}

/// GET /api/config — dashboard configuration.
pub async fn get_config(State(state): State<DashboardState>) -> Json<serde_json::Value> {
    let allowed = state.allowed_keys.lock().await;
    Json(json!({
        "max_workers": state.max_workers,
        "allow_list_enabled": !allowed.is_empty(),
        "allowed_key_count": allowed.len(),
    }))
}
