use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_ok = state.store.health_check().await;
    let pool = state.store.health_check_detailed();
    Json(json!({
        "name": "MineGraph",
        "version": minegraph_types::VERSION,
        "protocol_version": minegraph_types::PROTOCOL_VERSION,
        "build_commit": minegraph_types::BUILD_COMMIT,
        "status": if db_ok { "ok" } else { "degraded" },
        "db": if db_ok { "connected" } else { "error" },
        "server_key_id": state.server_identity.key_id.as_str(),
        "pool": {
            "size": pool.pool_size,
            "idle": pool.pool_idle,
        },
    }))
}
