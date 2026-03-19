use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct HistoryParams {
    pub since: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    500
}

/// GET /api/leaderboards/:n/history — leaderboard score history.
pub async fn get_history(
    State(state): State<AppState>,
    Path(n): Path<i32>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Value>, ApiError> {
    let since = params
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let snapshots = state.store.get_snapshots(n, since, params.limit).await?;

    let result: Vec<Value> = snapshots
        .iter()
        .map(|s| {
            json!({
                "t": s.snapshot_at.to_rfc3339(),
                "count": s.entry_count,
                "best_gap": s.best_gap,
                "worst_gap": s.worst_gap,
                "median_gap": s.median_gap,
                "avg_gap": s.avg_gap,
                "best_aut": s.best_aut,
                "avg_aut": s.avg_aut,
                "k4_red": s.total_k4_red,
                "k4_blue": s.total_k4_blue,
                "k5_red": s.total_k5_red,
                "k5_blue": s.total_k5_blue,
            })
        })
        .collect();

    Ok(Json(json!({ "n": n, "snapshots": result })))
}

/// GET /api/leaderboards/:n/export — download leaderboard as graph6.
pub async fn export_graph6(
    State(state): State<AppState>,
    Path(n): Path<i32>,
) -> Result<String, ApiError> {
    let graphs = state.store.export_leaderboard_graph6(n).await?;
    Ok(graphs.join("\n"))
}

/// GET /api/leaderboards/:n/export/csv — download leaderboard as CSV.
pub async fn export_csv(
    State(state): State<AppState>,
    Path(n): Path<i32>,
) -> Result<String, ApiError> {
    let entries = state.store.get_leaderboard_rich(n, 10000, 0).await?;
    let mut csv = String::from("rank,cid,graph6,goodman_gap,aut_order,key_id,admitted_at\n");
    for e in &entries {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            e.rank,
            e.cid,
            e.graph6,
            e.goodman_gap.unwrap_or(0.0),
            e.aut_order.unwrap_or(1.0),
            e.key_id,
            e.admitted_at.to_rfc3339(),
        ));
    }
    Ok(csv)
}
