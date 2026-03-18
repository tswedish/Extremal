use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_ok = state.store.health_check().await;
    Json(json!({
        "name": "MineGraph",
        "version": minegraph_types::PROTOCOL_VERSION,
        "status": if db_ok { "ok" } else { "degraded" },
        "db": if db_ok { "connected" } else { "error" },
        "server_key_id": state.server_identity.key_id.as_str(),
    }))
}
