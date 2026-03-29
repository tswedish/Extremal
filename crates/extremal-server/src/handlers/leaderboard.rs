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

/// GET /api/leaderboards/:n — paginated leaderboard with graph + score data.
pub async fn get_leaderboard(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, ApiError> {
    let entries = state
        .store
        .get_leaderboard_rich(n, params.limit, params.offset)
        .await?;
    let total = state.store.leaderboard_count(n).await?;

    let result: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "rank": e.rank,
                "cid": e.cid,
                "key_id": e.key_id,
                "graph6": e.graph6,
                "goodman_gap": e.goodman_gap,
                "aut_order": e.aut_order,
                "histogram": e.histogram,
                "admitted_at": e.admitted_at.to_rfc3339(),
            })
        })
        .collect();

    // Top graph is just the first entry
    let top_graph = entries.first().map(|e| {
        json!({
            "cid": e.cid,
            "graph6": e.graph6,
            "rank": e.rank,
        })
    });

    Ok(Json(json!({
        "n": n,
        "total": total,
        "entries": result,
        "top_graph": top_graph,
    })))
}

/// GET /api/leaderboards/:n/threshold — admission threshold (cached).
pub async fn get_threshold(
    State(state): State<AppState>,
    Path(n): Path<i32>,
) -> Result<Json<Value>, ApiError> {
    // Check cache first
    if let Some((count, threshold)) = state.cache.get_threshold(n).await {
        return Ok(Json(json!({
            "n": n,
            "count": count,
            "capacity": state.leaderboard_capacity,
            "threshold_score_bytes": threshold.map(hex::encode),
        })));
    }

    let count = state.store.leaderboard_count(n).await?;
    let threshold = state.store.leaderboard_threshold(n).await?;

    // Cache the result
    state.cache.set_threshold(n, count, threshold.clone()).await;

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

/// GET /api/leaderboards/:n/cids — incremental CID sync (cached for full sync).
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

    // Only cache full syncs (no `since` param) — incremental syncs are cheap
    if since.is_none()
        && let Some(cids) = state.cache.get_cids(n).await
    {
        return Ok(Json(json!({ "cids": cids })));
    }

    let cids = state.store.get_leaderboard_cids(n, since).await?;

    if since.is_none() {
        state.cache.set_cids(n, cids.clone()).await;
    }

    Ok(Json(json!({ "cids": cids })))
}

/// GET /api/leaderboards/:n/graphs — batch graph6 download (cached).
pub async fn get_graphs(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>, ApiError> {
    // Check cache first
    if let Some(cached) = state.cache.get_graphs(n, params.limit, params.offset).await {
        return Ok(Json(cached));
    }

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
    let response = json!({ "graphs": result });

    // Cache the result
    state
        .cache
        .set_graphs(n, params.limit, params.offset, response.clone())
        .await;

    Ok(Json(response))
}
