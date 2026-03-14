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
use ramseynet_verifier::scoring::compute_score_canonical;
use ramseynet_verifier::{canonical_form, verify_ramsey, VerifyRequest, VerifyResponse};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};

// ── Application state ────────────────────────────────────────────────

/// Shared application state threaded through all handlers.
pub struct AppState {
    pub ledger: Arc<Ledger>,
}

// ── Error mapping ────────────────────────────────────────────────────

type ApiError = (StatusCode, Json<Value>);

fn map_ledger_error(e: LedgerError) -> ApiError {
    match &e {
        LedgerError::GraphNotFound(_) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": e.to_string() })))
        }
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
async fn list_leaderboards(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, ApiError> {
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
    Ok(Json(json!({ "k": params.k, "ell": params.ell, "n_values": ns })))
}

/// GET /api/leaderboards/:k/:l/:n — full leaderboard for (k, ell, n).
async fn get_leaderboard(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
) -> Result<Json<Value>, ApiError> {
    let params = RamseyParams::canonical(k, ell);
    let ledger = state.ledger.clone();
    let (pk, pl, pn) = (params.k, params.ell, n);
    let entries = tokio::task::spawn_blocking(move || ledger.get_leaderboard(pk, pl, pn))
        .await
        .unwrap()
        .map_err(map_ledger_error)?;

    // Fetch RGXF for the top entry if present
    let top_graph: Option<Value> = if let Some(top) = entries.first() {
        let ledger2 = state.ledger.clone();
        let cid = top.graph_cid.clone();
        let rgxf_str = tokio::task::spawn_blocking(move || ledger2.get_submission_rgxf(&cid))
            .await
            .unwrap()
            .map_err(map_ledger_error)?;
        rgxf_str.and_then(|s| serde_json::from_str(&s).ok())
    } else {
        None
    };

    debug!(
        k = params.k, ell = params.ell, n,
        entries = entries.len(),
        "serving leaderboard detail"
    );

    Ok(Json(json!({
        "k": params.k,
        "ell": params.ell,
        "n": n,
        "entries": entries,
        "top_graph": top_graph,
    })))
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

/// GET /api/leaderboards/:k/:l/:n/graphs — RGXF for top leaderboard entries.
async fn get_leaderboard_graphs(
    State(state): State<Arc<AppState>>,
    Path((k, ell, n)): Path<(u32, u32, u32)>,
    Query(params): Query<GraphsQuery>,
) -> Result<Json<Value>, ApiError> {
    let rp = RamseyParams::canonical(k, ell);
    let limit = params.limit.unwrap_or(10).min(100);
    let ledger = state.ledger.clone();
    let (pk, pl) = (rp.k, rp.ell);
    let rgxfs = tokio::task::spawn_blocking(move || {
        ledger.get_leaderboard_graphs(pk, pl, n, limit)
    })
    .await
    .unwrap()
    .map_err(map_ledger_error)?;

    let graphs: Vec<Value> = rgxfs
        .into_iter()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();

    debug!(k = rp.k, ell = rp.ell, n, count = graphs.len(), "serving leaderboard graphs");

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
}

// ── Verify ──────────────────────────────────────────────────────────

/// Stateless verification — no database interaction.
/// Returns the canonical CID (isomorphism-class identity) when want_cid is true.
async fn verify(
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
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
}

/// Full lifecycle: verify + canonicalize + store + leaderboard admission.
///
/// The submitted graph is canonicalized via nauty before storage.
/// The canonical CID is the submission's identity — isomorphic graphs
/// map to the same CID and are deduplicated automatically.
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
            Json(json!({ "error": format!("n mismatch: graph has {} vertices but n={}", adj.n(), n) })),
        ));
    }

    // 2. Compute canonical form and canonical CID
    //    The canonical CID is isomorphism-invariant: two isomorphic graphs
    //    produce the same CID and are treated as duplicates.
    let (canonical_adj, _aut_order) = canonical_form(&adj);
    let canonical_cid = compute_cid(&canonical_adj);
    let cid_hex = canonical_cid.to_hex();

    info!(
        graph_cid = %cid_hex,
        k, ell, n,
        edges = adj.num_edges(),
        "received submission (canonical CID)"
    );

    // 3. Verify the original graph (verification is label-independent)
    let result = verify_ramsey(&adj, k, ell, &canonical_cid);

    info!(
        graph_cid = %cid_hex,
        verdict = %result.verdict,
        reason = ?result.reason,
        "verified graph"
    );

    // 4. Store submission using canonical RGXF and canonical CID
    let canonical_rgxf = rgxf::to_json(&canonical_adj);
    let rgxf_json_str = serde_json::to_string(&canonical_rgxf).unwrap();
    let ledger = state.ledger.clone();
    let cid_hex2 = cid_hex.clone();
    let is_duplicate = {
        let result = tokio::task::spawn_blocking(move || {
            ledger.store_submission(k, ell, &cid_hex2, n, &rgxf_json_str)
        })
        .await
        .unwrap();
        match result {
            Ok(_) => false,
            Err(LedgerError::GraphAlreadySubmitted(_)) => {
                info!(graph_cid = %cid_hex, "duplicate submission (isomorphic graph already stored)");
                true
            }
            Err(e) => return Err(map_ledger_error(e)),
        }
    };

    // 5. Store verification receipt (skip if duplicate)
    let verdict_str = result.verdict.to_string();
    let reason = result.reason.clone();
    let witness = result.witness.clone();

    if !is_duplicate {
        let ledger = state.ledger.clone();
        let cid_hex3 = cid_hex.clone();
        let verdict2 = verdict_str.clone();
        let reason2 = reason.clone();
        let witness2 = witness.clone();
        tokio::task::spawn_blocking(move || {
            ledger.store_receipt(
                &cid_hex3,
                k,
                ell,
                &verdict2,
                reason2.as_deref(),
                witness2.as_deref(),
            )
        })
        .await
        .unwrap()
        .map_err(map_ledger_error)?;
    }

    // 6. If accepted, compute score (canonical form) and try to admit
    let mut admitted = false;
    let mut rank: Option<u32> = None;
    let mut score_json: Option<Value> = None;

    if verdict_str == "accepted" {
        // Score uses canonical form — nauty is called once (already done above
        // for canonical_form, but compute_score_canonical does its own call
        // to get clique counts too). This is correct since clique counts are
        // isomorphism-invariant.
        let adj2 = adj.clone();
        let score_result = tokio::task::spawn_blocking(move || {
            compute_score_canonical(&adj2)
        })
        .await
        .unwrap();

        let graph_score = &score_result.score;

        let admit_score = AdmitScore {
            tier1_max: graph_score.c_omega.max(graph_score.c_alpha),
            tier1_min: graph_score.c_omega.min(graph_score.c_alpha),
            tier2_aut: graph_score.aut_order,
            tier3_cid: cid_hex.clone(),
            score_json: serde_json::to_string(graph_score).unwrap(),
        };

        let ledger = state.ledger.clone();
        let cid_hex4 = cid_hex.clone();
        let entry = tokio::task::spawn_blocking(move || {
            ledger.try_admit(k, ell, n, &cid_hex4, &admit_score)
        })
        .await
        .unwrap()
        .map_err(map_ledger_error)?;

        if let Some(entry) = entry {
            admitted = true;
            rank = Some(entry.rank);
            score_json = serde_json::from_str(&entry.score_json).ok();

            info!(
                graph_cid = %cid_hex,
                k, ell, n,
                rank = entry.rank,
                "admitted to leaderboard"
            );
        } else {
            warn!(
                graph_cid = %cid_hex,
                k, ell, n,
                "not admitted — below leaderboard threshold"
            );
        }
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
            "reason": reason,
            "witness": witness,
            "admitted": admitted,
            "rank": rank,
            "score": score_json,
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
        .route("/api/submissions/{cid}", get(get_submission))
        .route("/api/verify", post(verify))
        .route("/api/submit", post(submit_graph))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
