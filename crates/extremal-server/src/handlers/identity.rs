use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use extremal_identity::compute_key_id_from_hex;

use tracing::info;

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

    info!(
        key_id = %identity.key_id,
        display_name = identity.display_name.as_deref().unwrap_or("-"),
        "key registered"
    );

    Ok(Json(json!({
        "key_id": identity.key_id,
        "display_name": identity.display_name,
        "github_repo": identity.github_repo,
        "created_at": identity.created_at.to_rfc3339(),
    })))
}

#[derive(Deserialize)]
pub struct GetKeyParams {
    #[serde(default = "default_lb_limit")]
    pub leaderboard_limit: usize,
}

fn default_lb_limit() -> usize {
    10
}

/// GET /api/keys/:key_id — look up identity.
pub async fn get_key(
    State(state): State<AppState>,
    Path(key_id): Path<String>,
    Query(params): Query<GetKeyParams>,
) -> Result<Json<Value>, ApiError> {
    let identity = state
        .store
        .get_identity(&key_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("key {key_id}")))?;

    let (sub_count, _) = state.store.get_identity_stats(&key_id).await?;
    let lb_entries = state
        .store
        .get_identity_leaderboard_entries(&key_id)
        .await?;

    let total_leaderboard_entries = lb_entries.len();

    // Summary stats across all entries
    let n_values: Vec<i32> = lb_entries.iter().map(|e| e.n).collect();
    let best_rank = lb_entries.iter().map(|e| e.rank).min();
    let avg_rank = if !lb_entries.is_empty() {
        Some(lb_entries.iter().map(|e| e.rank as f64).sum::<f64>() / lb_entries.len() as f64)
    } else {
        None
    };

    // Return only top entries (sorted by rank across all n values)
    let mut sorted = lb_entries;
    sorted.sort_by_key(|e| e.rank);
    let top_entries: Vec<Value> = sorted
        .iter()
        .take(params.leaderboard_limit)
        .map(|e| {
            json!({
                "n": e.n,
                "rank": e.rank,
                "cid": e.cid,
                "graph6": e.graph6,
                "goodman_gap": e.goodman_gap,
                "aut_order": e.aut_order,
            })
        })
        .collect();

    Ok(Json(json!({
        "key_id": identity.key_id,
        "public_key": identity.public_key,
        "display_name": identity.display_name,
        "github_repo": identity.github_repo,
        "created_at": identity.created_at.to_rfc3339(),
        "total_submissions": sub_count,
        "leaderboard_entries": top_entries,
        "leaderboard_summary": {
            "total_entries": total_leaderboard_entries,
            "n_values": n_values,
            "best_rank": best_rank,
            "avg_rank": avg_rank,
        },
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
