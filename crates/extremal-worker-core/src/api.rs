//! Worker HTTP API for runtime control.
//!
//! Exposes endpoints for querying status, reading/updating config,
//! and controlling the engine (pause/resume/stop).

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{error, info};

use crate::engine::{EngineCommand, EngineSnapshot};

#[derive(Clone)]
struct ApiState {
    cmd_tx: mpsc::Sender<EngineCommand>,
    snapshot: watch::Receiver<EngineSnapshot>,
}

/// Start the worker HTTP API server. Returns the actual bound address.
pub async fn run_api_server(
    addr: SocketAddr,
    cmd_tx: mpsc::Sender<EngineCommand>,
    snapshot: watch::Receiver<EngineSnapshot>,
) -> Result<SocketAddr, std::io::Error> {
    let state = ApiState { cmd_tx, snapshot };

    let app = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/config", get(get_config))
        .route("/api/config", post(update_config))
        .route("/api/pause", post(pause))
        .route("/api/resume", post(resume))
        .route("/api/stop", post(stop))
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    info!(%actual_addr, "worker API server started");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("worker API server error: {e}");
        }
    });

    Ok(actual_addr)
}

async fn get_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let snap = state.snapshot.borrow().clone();
    Json(json!({
        "state": snap.state,
        "round": snap.round,
        "n": snap.n,
        "strategy": snap.strategy,
        "worker_id": snap.worker_id,
        "key_id": snap.key_id,
        "metrics": snap.metrics,
    }))
}

async fn get_config(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let snap = state.snapshot.borrow().clone();
    Json(json!({
        "params": snap.config.params,
    }))
}

async fn update_config(
    State(state): State<ApiState>,
    Json(patch): Json<HashMap<String, serde_json::Value>>,
) -> impl IntoResponse {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state
        .cmd_tx
        .send(EngineCommand::UpdateConfig {
            patch,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "engine not running"})),
        );
    }

    match reply_rx.await {
        Ok(result) => (StatusCode::OK, Json(json!(result))),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({"error": "engine did not respond"})),
        ),
    }
}

async fn pause(State(state): State<ApiState>) -> impl IntoResponse {
    if state.cmd_tx.send(EngineCommand::Pause).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "engine not running"})),
        );
    }
    (StatusCode::OK, Json(json!({"ok": true, "action": "pause"})))
}

async fn resume(State(state): State<ApiState>) -> impl IntoResponse {
    if state.cmd_tx.send(EngineCommand::Resume).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "engine not running"})),
        );
    }
    (
        StatusCode::OK,
        Json(json!({"ok": true, "action": "resume"})),
    )
}

async fn stop(State(state): State<ApiState>) -> impl IntoResponse {
    if state.cmd_tx.send(EngineCommand::Stop).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "engine not running"})),
        );
    }
    (StatusCode::OK, Json(json!({"ok": true, "action": "stop"})))
}
