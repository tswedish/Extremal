//! Axum-based viz server with embedded HTML page and WebSocket streaming.
//! Handles both outgoing viz data and incoming worker commands.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use ramseynet_worker_api::{WorkerCommand, WorkerEvent};
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use super::{VizHandle, VizMessage};

const PAGE_HTML: &str = include_str!("page.html");

struct AppState {
    viz: Arc<VizHandle>,
    cmd_tx: mpsc::Sender<WorkerCommand>,
    event_rx: watch::Receiver<Option<WorkerEvent>>,
    strategies: Vec<ramseynet_worker_api::StrategyInfo>,
}

pub async fn start_viz_server(
    port: u16,
    viz: Arc<VizHandle>,
    cmd_tx: mpsc::Sender<WorkerCommand>,
    event_rx: watch::Receiver<Option<WorkerEvent>>,
    strategies: Vec<ramseynet_worker_api::StrategyInfo>,
    mut shutdown: watch::Receiver<bool>,
) {
    let state = Arc::new(AppState {
        viz,
        cmd_tx,
        event_rx,
        strategies,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("failed to bind viz server");

    info!("viz server listening on http://localhost:{port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.wait_for(|v| *v).await;
        })
        .await
        .expect("viz server error");
}

async fn index_handler() -> impl IntoResponse {
    Html(PAGE_HTML)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    // Send hello
    let hello = VizMessage::Hello {
        version: crate::WORKER_VERSION.to_string(),
    };
    if send_json(&mut socket, &hello).await.is_err() {
        return;
    }

    // Send available strategies with config schemas
    if !state.strategies.is_empty() {
        let strat_msg = VizMessage::Strategies {
            strategies: state.strategies.clone(),
        };
        if send_json(&mut socket, &strat_msg).await.is_err() {
            return;
        }
    }

    // Send current leaderboard state so reconnecting browsers see the full board
    let lb_rx = state.viz.subscribe_leaderboard();
    let current_lb = lb_rx.borrow().clone();
    if !current_lb.is_empty() {
        let lb_msg = VizMessage::Leaderboard {
            entries: current_lb,
        };
        if send_json(&mut socket, &lb_msg).await.is_err() {
            return;
        }
    }

    // Request current status so the UI initializes correctly
    let _ = state.cmd_tx.send(WorkerCommand::Status).await;

    let mut snapshot_rx = state.viz.subscribe_snapshot();
    let mut lb_rx = state.viz.subscribe_leaderboard();
    let mut event_rx = state.event_rx.clone();
    let mut interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let snapshot = snapshot_rx.borrow_and_update().clone();
                if let Some(snap) = snapshot {
                    let msg = VizMessage::Snapshot(snap);
                    if send_json(&mut socket, &msg).await.is_err() {
                        break;
                    }
                }
            }
            result = lb_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let entries = lb_rx.borrow_and_update().clone();
                let msg = VizMessage::Leaderboard { entries };
                if send_json(&mut socket, &msg).await.is_err() {
                    break;
                }
            }
            result = event_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let event = event_rx.borrow_and_update().clone();
                if let Some(event) = event {
                    let msg = match event {
                        WorkerEvent::Status(status) => VizMessage::Status(status),
                        WorkerEvent::Error { message } => VizMessage::Error { message },
                        WorkerEvent::Strategies { strategies } => VizMessage::Strategies { strategies },
                    };
                    if send_json(&mut socket, &msg).await.is_err() {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<WorkerCommand>(&text) {
                            Ok(cmd) => {
                                debug!(?cmd, "received command from UI");
                                if let Err(e) = state.cmd_tx.send(cmd).await {
                                    warn!("failed to send command: {e}");
                                }
                            }
                            Err(e) => {
                                debug!("ignoring invalid command: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

async fn send_json<T: serde::Serialize>(socket: &mut WebSocket, msg: &T) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}
