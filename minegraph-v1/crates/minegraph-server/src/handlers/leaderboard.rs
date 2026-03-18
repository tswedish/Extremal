use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// GET /api/leaderboards — list all n values with summary.
pub async fn list_leaderboards(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let summaries = state.store.list_leaderboard_ns().await?;
    let result: Vec<Value> = summaries
        .iter()
        .map(|s| {
            json!({
                "n": s.n,
                "entry_count": s.entry_count,
            })
        })
        .collect();
    Ok(Json(json!({ "leaderboards": result })))
}

/// GET /api/leaderboards/:n — paginated leaderboard.
pub async fn get_leaderboard(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, ApiError> {
    let entries = state
        .store
        .get_leaderboard(n, params.limit, params.offset)
        .await?;
    let total = state.store.leaderboard_count(n).await?;

    // Get the top graph's graph6 for visualization
    let top_graph = if params.offset == 0 {
        state
            .store
            .get_leaderboard_graphs(n, 1, 0)
            .await?
            .into_iter()
            .next()
    } else {
        None
    };

    let result: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "rank": e.rank,
                "cid": e.cid,
                "key_id": e.key_id,
                "admitted_at": e.admitted_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(json!({
        "n": n,
        "total": total,
        "entries": result,
        "top_graph": top_graph.map(|g| json!({
            "cid": g.cid,
            "graph6": g.graph6,
            "rank": g.rank,
        })),
    })))
}

/// GET /api/leaderboards/:n/threshold — admission threshold.
pub async fn get_threshold(
    State(state): State<AppState>,
    Path(n): Path<i32>,
) -> Result<Json<Value>, ApiError> {
    let count = state.store.leaderboard_count(n).await?;
    let threshold = state.store.leaderboard_threshold(n).await?;
    Ok(Json(json!({
        "n": n,
        "count": count,
        "capacity": state.leaderboard_capacity,
        "threshold_score_bytes": threshold.map(hex::encode),
    })))
}

#[derive(Deserialize)]
pub struct CidParams {
    pub since: Option<String>,
}

/// GET /api/leaderboards/:n/cids — incremental CID sync.
pub async fn get_cids(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<CidParams>,
) -> Result<Json<Value>, ApiError> {
    let since = params
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let cids = state.store.get_leaderboard_cids(n, since).await?;
    Ok(Json(json!({ "cids": cids })))
}

/// GET /api/leaderboards/:n/graphs — batch graph6 download.
pub async fn get_graphs(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, ApiError> {
    let graphs = state
        .store
        .get_leaderboard_graphs(n, params.limit, params.offset)
        .await?;
    let result: Vec<Value> = graphs
        .iter()
        .map(|g| {
            json!({
                "rank": g.rank,
                "cid": g.cid,
                "graph6": g.graph6,
            })
        })
        .collect();
    Ok(Json(json!({ "graphs": result })))
}
