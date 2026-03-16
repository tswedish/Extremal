use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use ramseynet_graph::{compute_cid, rgxf, RgxfJson};
use ramseynet_ledger::{AdmitScore, Ledger, LedgerError};
use ramseynet_types::RamseyParams;
use ramseynet_verifier::scoring::verify_and_score;
use ramseynet_verifier::{canonical_form, verify_ramsey, VerifyRequest, VerifyResponse};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, info};

// ── Application state ────────────────────────────────────────────────

/// Shared application state threaded through all handlers.
pub struct AppState {
    pub ledger: Arc<Ledger>,
}

// ── Error mapping ────────────────────────────────────────────────────

type ApiError = (StatusCode, Json<Value>);

fn map_ledger_error(e: LedgerError) -> ApiError {
    match &e {
        LedgerError::GraphNotFound(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        ),
        LedgerError::BelowThreshold(_, _, _) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": e.to_string() })),
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn health() -> Json<Value> {
    Json(json!({
        "name": "RamseyNet",
        "version": ramseynet_types::PROTOCOL_VERSION,
        "status": "ok"
    }))
}

// ── Leaderboard routes ──────────────────────────────────────────────

/// GET /api/leaderboards — list all (k, ell, n) leaderboards.
async fn list_leaderboards(State(state): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let ledger = state.ledger.clone();
    let summaries = tokio::task::spawn_blocking(move || ledger.list_leaderboards())
        .await
        .unwrap()
        .map_err(map_ledger_error)?;
    debug!(count = summaries.len(), "listing leaderboards");
    Ok(Json(json!({ "leaderboards": summaries })))
}

/// GET /api/leaderboards/:k/:l — list available n values for a (k, ell) pair.
async fn list_n_for_pair(
    State(state): State<Arc<AppState>>,
    Path((k, ell)): Path<(u32, u32)>,
) -> Result<Json<Value>, ApiError> {
    let params = RamseyParams::canonical(k, ell);
    let ledger = state.ledger.clone();
    let ns = tokio::task::spawn_blocking(move || ledger.list_n_for_pair(params.k, params.ell))
        .await
        .unwrap()
        .map_err(map_ledger_error)?;
    debug!(k = params.k, ell = params.ell, n_values = ?ns, "listing n values for pair");
    Ok(Json(
        json!({ "k": params.k, "ell": params.ell, "n_values": ns }),
    ))
}

/// GET /api/leaderboards/:k/:l/:n — paginated leaderboard for (k, ell, n).
/// Query params: ?offset=0&limit=50  (default limit=50, max 200)
async fn get_leaderboard(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<Json<Value>, ApiError> {
    let params = RamseyParams::canonical(k, ell);
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(50).min(200);
    let ledger = state.ledger.clone();
    let (pk, pl, pn) = (params.k, params.ell, n);
    let page =
        tokio::task::spawn_blocking(move || ledger.get_leaderboard_page(pk, pl, pn, offset, limit))
            .await
            .unwrap()
            .map_err(map_ledger_error)?;

    // Fetch RGXF for the top entry only on the first page
    let top_graph: Option<Value> = if offset == 0 {
        if let Some(top) = page.entries.first() {
            let ledger2 = state.ledger.clone();
            let cid = top.graph_cid.clone();
            let rgxf_str = tokio::task::spawn_blocking(move || ledger2.get_submission_rgxf(&cid))
                .await
                .unwrap()
                .map_err(map_ledger_error)?;
            rgxf_str.and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    } else {
        None
    };

    debug!(
        k = params.k,
        ell = params.ell,
        n,
        total = page.total,
        offset,
        limit,
        entries = page.entries.len(),
        "serving leaderboard page"
    );

    Ok(Json(json!({
        "k": params.k,
        "ell": params.ell,
        "n": n,
        "total": page.total,
        "offset": page.offset,
        "limit": page.limit,
        "entries": page.entries,
        "top_graph": top_graph,
    })))
}

#[derive(Deserialize)]
struct LeaderboardQuery {
    offset: Option<u32>,
    limit: Option<u32>,
}

/// GET /api/leaderboards/:k/:l/:n/threshold — admission threshold.
async fn get_threshold(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
) -> Result<Json<Value>, ApiError> {
    let params = RamseyParams::canonical(k, ell);
    let ledger = state.ledger.clone();
    let (pk, pl, pn) = (params.k, params.ell, n);
    let info = tokio::task::spawn_blocking(move || ledger.get_threshold(pk, pl, pn))
        .await
        .unwrap()
        .map_err(map_ledger_error)?;
    debug!(k = params.k, ell = params.ell, n, "serving threshold");
    Ok(Json(json!(info)))
}

/// GET /api/leaderboards/:k/:l/:n/graphs — RGXF for leaderboard entries.
/// Query params: ?limit=10&offset=0  (default limit=10, max 200)
async fn get_leaderboard_graphs(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
    Query(params): Query<GraphsQuery>,
) -> Result<Json<Value>, ApiError> {
    let rp = RamseyParams::canonical(k, ell);
    let limit = params.limit.unwrap_or(10).min(200);
    let offset = params.offset.unwrap_or(0);
    let ledger = state.ledger.clone();
    let (pk, pl) = (rp.k, rp.ell);
    let rgxfs = tokio::task::spawn_blocking(move || {
        ledger.get_leaderboard_graphs(pk, pl, n, limit, offset)
    })
    .await
    .unwrap()
    .map_err(map_ledger_error)?;

    let graphs: Vec<Value> = rgxfs
        .into_iter()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();

    debug!(
        k = rp.k,
        ell = rp.ell,
        n,
        count = graphs.len(),
        offset,
        "serving leaderboard graphs"
    );

    Ok(Json(json!({
        "k": rp.k,
        "ell": rp.ell,
        "n": n,
        "graphs": graphs,
    })))
}

#[derive(Deserialize)]
struct GraphsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

/// GET /api/leaderboards/:k/:l/:n/cids — incremental CID sync for workers.
/// Query params: ?since=<ISO8601>  (omit for full sync)
async fn get_leaderboard_cids(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
    Query(params): Query<CidsQuery>,
) -> Result<Json<Value>, ApiError> {
    let rp = RamseyParams::canonical(k, ell);
    let since = params.since.clone();
    let ledger = state.ledger.clone();
    let (pk, pl) = (rp.k, rp.ell);
    let (cids, total, last_updated) =
        tokio::task::spawn_blocking(move || ledger.get_cids_since(pk, pl, n, since.as_deref()))
            .await
            .unwrap()
            .map_err(map_ledger_error)?;

    debug!(
        k = rp.k, ell = rp.ell, n,
        since = ?params.since,
        cids = cids.len(), total,
        "serving leaderboard CIDs"
    );

    Ok(Json(json!({
        "k": rp.k,
        "ell": rp.ell,
        "n": n,
        "total": total,
        "cids": cids,
        "last_updated": last_updated,
    })))
}

#[derive(Deserialize)]
struct CidsQuery {
    since: Option<String>,
}

// ── Verify ──────────────────────────────────────────────────────────

/// Stateless verification — no database interaction.
/// Returns the canonical CID (isomorphism-class identity) when want_cid is true.
async fn verify(Json(req): Json<VerifyRequest>) -> Result<Json<VerifyResponse>, ApiError> {
    let adj = rgxf::from_json(&req.graph).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid RGXF: {e}") })),
        )
    })?;

    // Compute canonical form to get the isomorphism-invariant CID
    let (canonical_adj, _aut_order) = canonical_form(&adj);
    let cid = compute_cid(&canonical_adj);
    let result = verify_ramsey(&adj, req.k, req.ell, &cid);

    let mut response: VerifyResponse = result.into();
    if !req.want_cid {
        response.graph_cid = None;
    }

    Ok(Json(response))
}

// ── Submit ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SubmitRequest {
    k: u32,
    ell: u32,
    n: u32,
    graph: RgxfJson,
    /// Optional signing key ID (first 16 hex chars of SHA-256 of public key).
    key_id: Option<String>,
    /// Optional Ed25519 signature over the canonical submission payload.
    #[serde(default)]
    #[allow(dead_code)] // Will be used for signature verification in Phase 2
    signature: Option<String>,
}

/// Full lifecycle: verify + canonicalize + store + leaderboard admission.
///
/// Optimized pipeline with just 2 blocking dispatches:
///   1. CPU: canonical_form + verify + score (single complement, single nauty)
///   2. DB:  store_submission + store_receipt + try_admit (single transaction)
async fn submit_graph(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubmitRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let params = RamseyParams::canonical(req.k, req.ell);
    let k = params.k;
    let ell = params.ell;
    let n = req.n;

    // 1. Decode RGXF and validate graph size matches n
    let adj = rgxf::from_json(&req.graph).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid RGXF: {e}") })),
        )
    })?;

    if adj.n() != n {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": format!("n mismatch: graph has {} vertices but n={}", adj.n(), n) }),
            ),
        ));
    }

    // 2. CPU dispatch: canonical form + verify + score in a single pass
    //    One nauty call, one complement construction, shared clique data.
    let vsr = tokio::task::spawn_blocking(move || verify_and_score(&adj, k, ell))
        .await
        .unwrap();

    let cid_hex = vsr.canonical_cid.to_hex();
    let verdict_str = vsr.verdict.to_string();

    info!(
        graph_cid = %cid_hex,
        k, ell, n,
        verdict = %vsr.verdict,
        "verified + scored graph"
    );

    // 3. Prepare the canonical RGXF for storage
    let canonical_rgxf = rgxf::to_json(&vsr.canonical_graph);
    let rgxf_json_str = serde_json::to_string(&canonical_rgxf).unwrap();

    // Build admit score if the graph was accepted
    let admit_score = vsr.score.as_ref().map(|graph_score| AdmitScore {
        tier1_max: graph_score.c_omega.max(graph_score.c_alpha),
        tier1_min: graph_score.c_omega.min(graph_score.c_alpha),
        goodman_gap: graph_score.goodman_gap,
        tier2_aut: graph_score.aut_order,
        tier3_cid: cid_hex.clone(),
        score_json: serde_json::to_string(graph_score).unwrap(),
    });

    // 4. DB dispatch: store + receipt + admit in a single transaction
    let ledger = state.ledger.clone();
    let cid_hex2 = cid_hex.clone();
    let verdict2 = verdict_str.clone();
    let reason = vsr.reason.clone();
    let witness = vsr.witness.clone();
    let submitted_key_id = req.key_id.clone();
    let (is_duplicate, lb_entry) = tokio::task::spawn_blocking(move || {
        ledger.submit_and_admit(
            k,
            ell,
            n,
            &cid_hex2,
            &rgxf_json_str,
            &verdict2,
            reason.as_deref(),
            witness.as_deref(),
            admit_score.as_ref(),
            submitted_key_id.as_deref(),
        )
    })
    .await
    .unwrap()
    .map_err(map_ledger_error)?;

    if is_duplicate {
        info!(graph_cid = %cid_hex, "duplicate submission (isomorphic graph already stored)");
    }

    let admitted = lb_entry.is_some();
    let rank = lb_entry.as_ref().map(|e| e.rank);
    let score_json: Option<Value> = lb_entry
        .as_ref()
        .and_then(|e| serde_json::from_str(&e.score_json).ok());

    if let Some(ref entry) = lb_entry {
        if is_duplicate {
            debug!(
                graph_cid = %cid_hex,
                k, ell, n,
                rank = entry.rank,
                "duplicate — already on leaderboard"
            );
        } else {
            info!(
                graph_cid = %cid_hex,
                k, ell, n,
                rank = entry.rank,
                "admitted to leaderboard"
            );
        }
    } else if verdict_str == "accepted" {
        debug!(
            graph_cid = %cid_hex,
            k, ell, n,
            "not admitted — below leaderboard threshold"
        );
    }

    let status_code = if is_duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((
        status_code,
        Json(json!({
            "graph_cid": cid_hex,
            "verdict": verdict_str,
            "reason": vsr.reason,
            "witness": vsr.witness,
            "admitted": admitted,
            "rank": rank,
            "score": score_json,
            "key_id": req.key_id,
        })),
    ))
}

// ── Submission detail ───────────────────────────────────────────────

/// Get full submission detail by CID.
async fn get_submission(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    debug!(cid = %cid, "fetching submission detail");
    let ledger = state.ledger.clone();
    let detail = tokio::task::spawn_blocking(move || ledger.get_submission_detail(&cid))
        .await
        .unwrap()
        .map_err(map_ledger_error)?;

    let (submission, receipt, lb_entry) = detail.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Submission not found" })),
        )
    })?;

    let rgxf: Option<Value> = serde_json::from_str(&submission.rgxf_json).ok();

    Ok(Json(json!({
        "graph_cid": submission.graph_cid,
        "k": submission.k,
        "ell": submission.ell,
        "n": submission.n,
        "rgxf": rgxf,
        "submitted_at": submission.submitted_at,
        "verdict": receipt.as_ref().map(|r| &r.verdict),
        "reason": receipt.as_ref().and_then(|r| r.reason.as_ref()),
        "witness": receipt.as_ref().and_then(|r| r.witness.as_ref()),
        "verified_at": receipt.as_ref().map(|r| &r.verified_at),
        "leaderboard_rank": lb_entry.as_ref().map(|e| e.rank),
        "score": lb_entry.as_ref().and_then(|e| serde_json::from_str::<Value>(&e.score_json).ok()),
    })))
}

// ── Router ───────────────────────────────────────────────────────────

/// Create the application router with shared state.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .route("/api/leaderboards", get(list_leaderboards))
        .route("/api/leaderboards/{k}/{l}", get(list_n_for_pair))
        .route("/api/leaderboards/{k}/{l}/{n}", get(get_leaderboard))
        .route(
            "/api/leaderboards/{k}/{l}/{n}/threshold",
            get(get_threshold),
        )
        .route(
            "/api/leaderboards/{k}/{l}/{n}/graphs",
            get(get_leaderboard_graphs),
        )
        .route(
            "/api/leaderboards/{k}/{l}/{n}/cids",
            get(get_leaderboard_cids),
        )
        .route("/api/submissions/{cid}", get(get_submission))
        .route("/api/verify", post(verify))
        .route("/api/submit", post(submit_graph))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
