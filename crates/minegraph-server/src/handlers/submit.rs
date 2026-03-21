use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use minegraph_graph::{AdjacencyMatrix, graph6};
use minegraph_identity::{canonical_payload, verify_signature};
use minegraph_scoring::goodman;
use minegraph_scoring::histogram::CliqueHistogram;
use minegraph_scoring::score::GraphScore;

use tracing::info;

use crate::error::ApiError;
use crate::state::{AppState, ServerEvent};

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub n: u32,
    pub graph6: String,
    pub key_id: String,
    pub signature: String,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub n: u32,
    pub graph6: String,
}

/// POST /api/submit — full lifecycle: verify sig, score, store, admit.
pub async fn submit_graph(
    State(state): State<AppState>,
    Json(req): Json<SubmitRequest>,
) -> Result<Json<Value>, ApiError> {
    // 0a. Validate graph size
    if req.n == 0 || req.n > state.max_n {
        return Err(ApiError::BadRequest(format!(
            "n must be 1..={}",
            state.max_n
        )));
    }

    // 0b. Validate metadata size
    if let Some(ref meta) = req.metadata {
        let meta_str = serde_json::to_string(meta).unwrap_or_default();
        if meta_str.len() > 4096 {
            return Err(ApiError::BadRequest("metadata exceeds 4KB limit".into()));
        }
    }

    // 1. Decode graph
    let matrix = graph6::decode(&req.graph6)
        .map_err(|e| ApiError::BadRequest(format!("invalid graph6: {e}")))?;
    if matrix.n() != req.n {
        return Err(ApiError::BadRequest(format!(
            "graph6 decodes to n={}, expected n={}",
            matrix.n(),
            req.n
        )));
    }

    // 2. Verify identity exists
    let identity = state
        .store
        .get_identity(&req.key_id)
        .await?
        .ok_or_else(|| ApiError::UnregisteredIdentity(req.key_id.clone()))?;

    // 3. Verify signature
    let payload = canonical_payload(req.n, &req.graph6);
    let sig_valid = verify_signature(&identity.public_key, &payload, &req.signature)
        .map_err(|e| ApiError::BadRequest(format!("signature error: {e}")))?;
    if !sig_valid {
        return Err(ApiError::InvalidSignature);
    }

    // 4. Score the graph (CPU-intensive, run in blocking task)
    let max_k = state.max_k;
    let scored = tokio::task::spawn_blocking(move || score_graph(&matrix, max_k))
        .await
        .map_err(|e| ApiError::Internal(format!("scoring task failed: {e}")))?;

    let cid_hex = scored.score.cid.to_hex();

    // 5. Store graph (canonical graph6) + score
    state
        .store
        .store_graph(&cid_hex, req.n as i32, &scored.canonical_graph6)
        .await?;

    let histogram_json = serde_json::to_value(&scored.score.histogram)
        .map_err(|e| ApiError::Internal(format!("serialize histogram: {e}")))?;
    let score_bytes = scored.score.to_score_bytes(max_k);

    state
        .store
        .store_score(
            &cid_hex,
            req.n as i32,
            &histogram_json,
            scored.score.goodman_gap as f64,
            scored.score.aut_order,
            &score_bytes,
        )
        .await?;

    // 6. Store submission
    state
        .store
        .store_submission(&cid_hex, &req.key_id, &req.signature, req.metadata.as_ref())
        .await?;

    // 7. Try to admit to leaderboard
    let admitted = state
        .store
        .try_admit(
            req.n as i32,
            &cid_hex,
            &req.key_id,
            &score_bytes,
            state.leaderboard_capacity,
        )
        .await?;

    // 8. Sign receipt
    let verdict = "accepted";
    let receipt_payload = format!("{}:{}:{}", cid_hex, verdict, req.n);
    let receipt_sig = state.server_identity.sign(receipt_payload.as_bytes());
    let server_key_id = state.server_identity.key_id.as_str();

    let score_json = json!({
        "histogram": histogram_json,
        "goodman_gap": scored.score.goodman_gap,
        "aut_order": scored.score.aut_order,
    });

    state
        .store
        .store_receipt(
            &cid_hex,
            server_key_id,
            verdict,
            Some(&score_json),
            &receipt_sig,
        )
        .await?;

    // 9. Broadcast event + log
    if let Some(rank) = admitted {
        info!(
            n = req.n,
            rank,
            cid = %cid_hex,
            key_id = %req.key_id,
            goodman_gap = scored.score.goodman_gap,
            aut_order = scored.score.aut_order,
            "admission"
        );
        let _ = state.events_tx.send(ServerEvent::Admission {
            n: req.n as i32,
            cid: cid_hex.clone(),
            rank,
            key_id: req.key_id.clone(),
            graph6: scored.canonical_graph6.clone(),
            goodman_gap: scored.score.goodman_gap as f64,
            aut_order: scored.score.aut_order,
            metadata: req.metadata.clone(),
        });
    } else {
        info!(
            n = req.n,
            cid = %cid_hex,
            key_id = %req.key_id,
            "submission (not admitted)"
        );
        let _ = state.events_tx.send(ServerEvent::Submission {
            n: req.n as i32,
            cid: cid_hex.clone(),
            key_id: req.key_id.clone(),
            metadata: req.metadata.clone(),
        });
    }

    Ok(Json(json!({
        "cid": cid_hex,
        "verdict": verdict,
        "admitted": admitted.is_some(),
        "rank": admitted,
        "receipt": {
            "server_key_id": server_key_id,
            "signature": receipt_sig,
            "score": score_json,
        },
    })))
}

/// POST /api/verify — stateless scoring (no DB write, no signature required).
pub async fn verify_graph(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<Value>, ApiError> {
    // Validate graph size
    if req.n == 0 || req.n > state.max_n {
        return Err(ApiError::BadRequest(format!(
            "n must be 1..={}",
            state.max_n
        )));
    }

    let matrix = graph6::decode(&req.graph6)
        .map_err(|e| ApiError::BadRequest(format!("invalid graph6: {e}")))?;
    if matrix.n() != req.n {
        return Err(ApiError::BadRequest(format!(
            "graph6 decodes to n={}, expected n={}",
            matrix.n(),
            req.n
        )));
    }

    let max_k = state.max_k;
    let scored = tokio::task::spawn_blocking(move || score_graph(&matrix, max_k))
        .await
        .map_err(|e| ApiError::Internal(format!("scoring task failed: {e}")))?;

    let histogram_json = serde_json::to_value(&scored.score.histogram)
        .map_err(|e| ApiError::Internal(format!("serialize: {e}")))?;

    Ok(Json(json!({
        "cid": scored.score.cid.to_hex(),
        "n": req.n,
        "histogram": histogram_json,
        "goodman_gap": scored.score.goodman_gap,
        "aut_order": scored.score.aut_order,
        "canonical_graph6": scored.canonical_graph6,
    })))
}

/// GET /api/submissions/:cid — submission detail.
pub async fn get_submission(
    State(state): State<AppState>,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let submission = state
        .store
        .get_submission(&cid)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("submission {cid}")))?;

    let graph = state.store.get_graph(&cid).await?;
    let score = state.store.get_score(&cid).await?;
    let receipt = state.store.get_receipt(&cid).await?;

    Ok(Json(json!({
        "submission": {
            "cid": submission.cid,
            "key_id": submission.key_id,
            "metadata": submission.metadata,
            "created_at": submission.created_at.to_rfc3339(),
        },
        "graph": graph.map(|g| json!({
            "n": g.n,
            "graph6": g.graph6,
        })),
        "score": score.map(|s| json!({
            "histogram": s.histogram,
            "goodman_gap": s.goodman_gap,
            "aut_order": s.aut_order,
        })),
        "receipt": receipt.map(|r| json!({
            "server_key_id": r.server_key_id,
            "verdict": r.verdict,
            "signature": r.signature,
            "score": r.score_json,
        })),
    })))
}

/// Result of scoring a graph, including canonical form.
struct ScoredGraph {
    score: GraphScore,
    canonical_graph6: String,
}

/// Score a graph (CPU-intensive, runs in blocking task).
///
/// Performs canonical labeling via nauty, computes the clique histogram,
/// Goodman gap, |Aut(G)|, and canonical CID. Returns the full score plus
/// the canonical graph6 encoding for storage.
fn score_graph(matrix: &AdjacencyMatrix, max_k: u32) -> ScoredGraph {
    // Clique histogram (isomorphism-invariant, so use original graph)
    let histogram = CliqueHistogram::compute(matrix, max_k);

    let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
    let gap = goodman::goodman_gap(matrix.n(), red_tri, blue_tri);

    // Canonical form + |Aut(G)| via nauty (single call)
    let (canonical, aut_order) = minegraph_scoring::automorphism::canonical_form(matrix);

    // CID from canonical graph6
    let canonical_g6 = minegraph_graph::graph6::encode(&canonical);
    let cid = minegraph_graph::compute_cid(&canonical);

    ScoredGraph {
        score: GraphScore::new(histogram, gap, aut_order, cid),
        canonical_graph6: canonical_g6,
    }
}
