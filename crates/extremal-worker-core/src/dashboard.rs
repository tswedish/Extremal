//! Dashboard WebSocket client for streaming worker telemetry.
//!
//! Connects outbound to the dashboard relay server and streams
//! progress, discovery, and round events. Supports Ed25519
//! challenge/response authentication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use extremal_dashboard::protocol::{ServerChallenge, WorkerMessage};
use extremal_identity::Identity;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Max queued messages before dropping. Keeps the buffer from growing unbounded
/// during bursts of discoveries.
const MAX_QUEUED_MESSAGES: usize = 64;

/// Dashboard client that maintains a WebSocket connection to the relay server.
#[derive(Clone)]
pub struct DashboardClient {
    tx: mpsc::Sender<WorkerMessage>,
    connected: Arc<AtomicBool>,
}

impl DashboardClient {
    /// Spawn a dashboard client that connects to the given URL.
    /// Returns the client handle and a task that should be spawned.
    #[allow(clippy::too_many_arguments)]
    pub fn connect(
        url: String,
        identity: Option<Arc<Identity>>,
        worker_id: String,
        n: u32,
        strategy: String,
        metadata: Option<serde_json::Value>,
        api_addr: Option<String>,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(MAX_QUEUED_MESSAGES);
        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = connected.clone();

        tokio::spawn(async move {
            run_connection(
                url,
                identity,
                worker_id,
                n,
                strategy,
                metadata,
                api_addr,
                rx,
                connected_clone,
                shutdown,
            )
            .await;
        });

        Self { tx, connected }
    }

    /// Send a message to the dashboard. Non-blocking, drops if buffer full or disconnected.
    pub fn send(&self, msg: WorkerMessage) {
        if self.connected.load(Ordering::Relaxed) {
            // try_send: drops the message if the channel is full rather than blocking
            let _ = self.tx.try_send(msg);
        }
    }

    /// Check if currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_connection(
    url: String,
    identity: Option<Arc<Identity>>,
    worker_id: String,
    n: u32,
    strategy: String,
    metadata: Option<serde_json::Value>,
    api_addr: Option<String>,
    mut rx: mpsc::Receiver<WorkerMessage>,
    connected: Arc<AtomicBool>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut backoff_ms = 1000u64;
    let max_backoff_ms = 30_000u64;

    // Pre-compute auth fields if identity is available
    let (public_key_hex, key_id) = if let Some(ref id) = identity {
        (
            Some(hex::encode(id.verifying_key().as_bytes())),
            id.key_id.as_str().to_string(),
        )
    } else {
        (None, String::new())
    };

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

                // 1. Read challenge nonce from server
                let nonce_hex = match read.next().await {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        match serde_json::from_str::<ServerChallenge>(&text) {
                            Ok(challenge) => {
                                debug!("received challenge nonce");
                                Some(challenge.nonce)
                            }
                            Err(_) => {
                                warn!(
                                    "unexpected first message (not a challenge), continuing without auth"
                                );
                                None
                            }
                        }
                    }
                    _ => {
                        warn!("no challenge from dashboard, reconnecting");
                        connected.store(false, Ordering::Relaxed);
                        continue;
                    }
                };

                // 2. Build Register message with auth fields
                let (sig_hex, pk_hex) = if let (Some(nonce), Some(id), Some(pk)) =
                    (&nonce_hex, &identity, &public_key_hex)
                {
                    // Sign the raw nonce bytes
                    match hex::decode(nonce) {
                        Ok(nonce_bytes) => {
                            let signature = id.sign(&nonce_bytes);
                            (Some(signature), Some(pk.clone()))
                        }
                        Err(e) => {
                            warn!("invalid nonce hex: {e}");
                            (None, None)
                        }
                    }
                } else {
                    (None, None)
                };

                let register_msg = WorkerMessage::Register {
                    key_id: key_id.clone(),
                    worker_id: worker_id.clone(),
                    n,
                    strategy: strategy.clone(),
                    metadata: metadata.clone(),
                    public_key_hex: pk_hex,
                    nonce_signature: sig_hex,
                    api_addr: api_addr.clone(),
                };

                // 3. Send registration
                if let Ok(json) = serde_json::to_string(&register_msg)
                    && write
                        .send(tokio_tungstenite::tungstenite::Message::Text(json.into()))
                        .await
                        .is_err()
                {
                    connected.store(false, Ordering::Relaxed);
                    continue;
                }

                // 4. Wait for ack
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
