use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use minegraph_identity::compute_key_id_from_hex;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct RegisterKeyRequest {
    pub public_key: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub github_repo: Option<String>,
}

/// POST /api/keys — register a public key.
pub async fn register_key(
    State(state): State<AppState>,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<Json<Value>, ApiError> {
    // Compute key_id from public key
    let key_id = compute_key_id_from_hex(&req.public_key)
        .map_err(|e| ApiError::BadRequest(format!("invalid public key: {e}")))?;

    let identity = state
        .store
        .register_identity(
            key_id.as_str(),
            &req.public_key,
            req.display_name.as_deref(),
            req.github_repo.as_deref(),
        )
        .await?;

    Ok(Json(json!({
        "key_id": identity.key_id,
        "display_name": identity.display_name,
        "github_repo": identity.github_repo,
        "created_at": identity.created_at.to_rfc3339(),
    })))
}

/// GET /api/keys/:key_id — look up identity.
pub async fn get_key(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let identity = state
        .store
        .get_identity(&key_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("key {key_id}")))?;

    Ok(Json(json!({
        "key_id": identity.key_id,
        "public_key": identity.public_key,
        "display_name": identity.display_name,
        "github_repo": identity.github_repo,
        "created_at": identity.created_at.to_rfc3339(),
    })))
}

#[derive(Deserialize)]
pub struct SubmissionParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// GET /api/keys/:key_id/submissions — submissions by identity.
pub async fn get_key_submissions(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
    Query(params): Query<SubmissionParams>,
) -> Result<Json<Value>, ApiError> {
    let submissions = state
        .store
        .get_submissions_by_identity(&key_id, params.limit, params.offset)
        .await?;

    let result: Vec<Value> = submissions
        .iter()
        .map(|s| {
            json!({
                "cid": s.cid,
                "metadata": s.metadata,
                "created_at": s.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(json!({
        "key_id": key_id,
        "submissions": result,
    })))
}
