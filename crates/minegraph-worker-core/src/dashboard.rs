//! Dashboard WebSocket client for streaming worker telemetry.
//!
//! Connects outbound to the dashboard relay server and streams
//! progress, discovery, and round events.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::{SinkExt, StreamExt};
use minegraph_dashboard::protocol::WorkerMessage;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Dashboard client that maintains a WebSocket connection to the relay server.
#[derive(Clone)]
pub struct DashboardClient {
    tx: mpsc::UnboundedSender<WorkerMessage>,
    connected: Arc<AtomicBool>,
}

impl DashboardClient {
    /// Spawn a dashboard client that connects to the given URL.
    /// Returns the client handle and a task that should be spawned.
    pub fn connect(
        url: String,
        register_msg: WorkerMessage,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = connected.clone();

        tokio::spawn(async move {
            run_connection(url, register_msg, rx, connected_clone, shutdown).await;
        });

        Self { tx, connected }
    }

    /// Send a message to the dashboard. Non-blocking, drops if disconnected.
    pub fn send(&self, msg: WorkerMessage) {
        if self.connected.load(Ordering::Relaxed) {
            let _ = self.tx.send(msg);
        }
    }

    /// Check if currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

async fn run_connection(
    url: String,
    register_msg: WorkerMessage,
    mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
    connected: Arc<AtomicBool>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut backoff_ms = 1000u64;
    let max_backoff_ms = 30_000u64;

    loop {
        if *shutdown.borrow() {
            break;
        }

        info!(url, "connecting to dashboard relay...");

        match tokio_tungstenite::connect_async(&url).await {
            Ok((ws_stream, _)) => {
                info!(url, "connected to dashboard relay");
                connected.store(true, Ordering::Relaxed);
                backoff_ms = 1000; // reset backoff

                let (mut write, mut read) = ws_stream.split();

                // Send registration
                if let Ok(json) = serde_json::to_string(&register_msg)
                    && write
                        .send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
                        .await
                        .is_err()
                {
                    connected.store(false, Ordering::Relaxed);
                    continue;
                }

                // Wait for ack
                match read.next().await {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        debug!("dashboard ack: {text}");
                    }
                    _ => {
                        warn!("no ack from dashboard, reconnecting");
                        connected.store(false, Ordering::Relaxed);
                        continue;
                    }
                }

                // Main relay loop: forward messages from engine to dashboard
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            match msg {
                                Some(worker_msg) => {
                                    if let Ok(json) = serde_json::to_string(&worker_msg)
                                        && write
                                            .send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
                                            .await
                                            .is_err()
                                    {
                                        break;
                                    }
                                }
                                None => break, // channel closed
                            }
                        }
                        ws_msg = read.next() => {
                            match ws_msg {
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(data))) => {
                                    let _ = write.send(tokio_tungstenite::tungstenite::Message::Pong(data)).await;
                                }
                                Some(Ok(_)) => {} // ignore commands for now
                                Some(Err(_)) => break,
                            }
                        }
                        _ = shutdown.changed() => {
                            if *shutdown.borrow() {
                                let _ = write.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                                connected.store(false, Ordering::Relaxed);
                                return;
                            }
                        }
                    }
                }

                connected.store(false, Ordering::Relaxed);
                warn!("dashboard connection lost, reconnecting...");
            }
            Err(e) => {
                debug!(url, "dashboard connection failed: {e}");
            }
        }

        // Exponential backoff
        if *shutdown.borrow() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(max_backoff_ms);

        // Drain any queued messages while disconnected
        while rx.try_recv().is_ok() {}
    }
}
