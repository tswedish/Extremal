//! Axum-based viz server with embedded HTML page and WebSocket streaming.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use tokio::sync::watch;
use tracing::{info, warn};

use super::{VizHandle, VizMessage};

const PAGE_HTML: &str = include_str!("page.html");

struct AppState {
    viz: Arc<VizHandle>,
}

pub async fn start_viz_server(
    port: u16,
    viz: Arc<VizHandle>,
    mut shutdown: watch::Receiver<bool>,
) {
    let state = Arc::new(AppState { viz });

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

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    // Send hello
    let hello = VizMessage::Hello {
        version: crate::SEARCH_VERSION.to_string(),
    };
    if send_json(&mut socket, &hello).await.is_err() {
        return;
    }

    let mut snapshot_rx = state.viz.subscribe_snapshot();
    let mut pinned_rx = state.viz.subscribe_pinned();
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
            result = pinned_rx.recv() => {
                match result {
                    Ok(pinned) => {
                        let msg = VizMessage::Pinned(pinned);
                        if send_json(&mut socket, &msg).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        // Lagged — skip
                        warn!("viz ws: pinned broadcast lagged");
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
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
