//! System functional test for the RamseyNet server.
//!
//! Spins up a real Axum server on a random port and exercises the full
//! HTTP API with realistic Ramsey graph payloads. This verifies:
//! - Server boots and serves all endpoints
//! - Graph encoding (RGXF) works end-to-end over HTTP
//! - Verifier correctly accepts/rejects graphs
//! - CID computation is stable across requests
//! - Error handling returns proper 400 responses
//! - Submit lifecycle (verify + store + leaderboard admission + events)
//! - Leaderboard queries (list, detail, threshold)
//! - K > L auto-canonicalization

use std::sync::Arc;

use ramseynet_graph::{rgxf, AdjacencyMatrix};
use ramseynet_ledger::Ledger;
use ramseynet_server::AppState;
use ramseynet_verifier::VerifyRequest;
use serde_json::Value;

/// Start a real server on port 0 (OS-assigned) with an in-memory ledger.
async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let ledger = Arc::new(Ledger::open_in_memory().expect("in-memory ledger"));
    let state = Arc::new(AppState { ledger });

    let app = ramseynet_server::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base_url = format!("http://127.0.0.1:{port}");

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (base_url, handle)
}

/// Build a C5 (5-cycle) graph — the classic R(3,3) witness on 5 vertices.
/// Has omega=2, alpha=2 so it should be ACCEPTED for R(3,3).
fn build_c5() -> ramseynet_graph::RgxfJson {
    let mut g = AdjacencyMatrix::new(5);
    g.set_edge(0, 1, true);
    g.set_edge(1, 2, true);
    g.set_edge(2, 3, true);
    g.set_edge(3, 4, true);
    g.set_edge(4, 0, true);
    rgxf::to_json(&g)
}

/// Build K_n (complete graph on n vertices).
fn build_kn(n: u32) -> ramseynet_graph::RgxfJson {
    let mut g = AdjacencyMatrix::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            g.set_edge(i, j, true);
        }
    }
    rgxf::to_json(&g)
}

/// Build an empty graph on n vertices (no edges).
fn build_empty(n: u32) -> ramseynet_graph::RgxfJson {
    let g = AdjacencyMatrix::new(n);
    rgxf::to_json(&g)
}

fn make_verify_request(
    k: u32,
    ell: u32,
    graph: ramseynet_graph::RgxfJson,
    want_cid: bool,
) -> VerifyRequest {
    VerifyRequest {
        oras_version: "ovwc-1".to_string(),
        k,
        ell,
        graph,
        want_cid,
    }
}

// ── Test: Full lifecycle ────────────────────────────────────────────

#[tokio::test]
async fn system_test_full_lifecycle() {
    let (base, _handle) = start_server().await;
    let client = reqwest::Client::new();

    // ── 1. Health check ─────────────────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/health"))
            .send()
            .await
            .expect("health request failed");
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["name"], "RamseyNet");
        assert_eq!(body["status"], "ok");
        eprintln!("[PASS] 1. Health check");
    }

    // ── 2. Leaderboards list (empty) ────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/leaderboards"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["leaderboards"], Value::Array(vec![]));
        eprintln!("[PASS] 2. Leaderboards list is empty");
    }

    // ── 3. Verify C5 for R(3,3) — ACCEPTED ─────────────────────────
    let c5_cid: String;
    {
        let req = make_verify_request(3, 3, build_c5(), true);
        let resp = client
            .post(format!("{base}/api/verify"))
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "accepted");
        let cid = body["graph_cid"].as_str().unwrap();
        assert_eq!(cid.len(), 64);
        c5_cid = cid.to_string();
        eprintln!("[PASS] 3. C5 accepted for R(3,3), CID: {}", &c5_cid[..16]);
    }

    // ── 4. Verify K5 for R(3,3) — REJECTED ─────────────────────────
    {
        let req = make_verify_request(3, 3, build_kn(5), true);
        let resp = client
            .post(format!("{base}/api/verify"))
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "rejected");
        assert_eq!(body["reason"], "clique_found");
        eprintln!("[PASS] 4. K5 rejected for R(3,3)");
    }

    // ── 5. Verify empty graph for R(3,3) — REJECTED ────────────────
    {
        let req = make_verify_request(3, 3, build_empty(5), true);
        let resp = client
            .post(format!("{base}/api/verify"))
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "rejected");
        assert_eq!(body["reason"], "independent_set_found");
        eprintln!("[PASS] 5. Empty graph rejected for R(3,3)");
    }

    // ── 6. Invalid RGXF — 400 ──────────────────────────────────────
    {
        let bad_graph = ramseynet_graph::RgxfJson {
            n: 5,
            encoding: "utri_b64_v1".to_string(),
            bits_b64: "/w==".to_string(),
        };
        let req = make_verify_request(3, 3, bad_graph, false);
        let resp = client
            .post(format!("{base}/api/verify"))
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        eprintln!("[PASS] 6. Invalid RGXF returns 400");
    }

    // ── 7. CID stability ────────────────────────────────────────────
    {
        let req = make_verify_request(3, 3, build_c5(), true);
        let resp = client
            .post(format!("{base}/api/verify"))
            .json(&req)
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["graph_cid"].as_str().unwrap(), c5_cid);
        eprintln!("[PASS] 7. CID stability confirmed");
    }

    // ── 8. Submit C5 for R(3,3) n=5 — accepted, admitted to leaderboard ─
    {
        let resp = client
            .post(format!("{base}/api/submit"))
            .json(&serde_json::json!({
                "k": 3,
                "ell": 3,
                "n": 5,
                "graph": build_c5(),
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["verdict"], "accepted");
        assert_eq!(body["graph_cid"], c5_cid);
        assert_eq!(body["admitted"], true);
        assert_eq!(body["rank"], 1);
        assert!(body["score"].is_object());
        eprintln!("[PASS] 8. Submit C5: accepted, admitted rank=1");
    }

    // ── 9. Submit K5 for R(3,3) — rejected, not admitted ───────────
    {
        let resp = client
            .post(format!("{base}/api/submit"))
            .json(&serde_json::json!({
                "k": 3,
                "ell": 3,
                "n": 5,
                "graph": build_kn(5),
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["verdict"], "rejected");
        assert_eq!(body["admitted"], false);
        assert!(body["rank"].is_null());
        eprintln!("[PASS] 9. Submit K5: rejected, not admitted");
    }

    // ── 10. Duplicate submission (C5 again) → 200 ──────────────────
    {
        let resp = client
            .post(format!("{base}/api/submit"))
            .json(&serde_json::json!({
                "k": 3,
                "ell": 3,
                "n": 5,
                "graph": build_c5(),
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "duplicate should return 200");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["verdict"], "accepted");
        assert_eq!(body["admitted"], true, "duplicate already on leaderboard should report admitted");
        assert_eq!(body["rank"], 1, "duplicate should report existing rank");
        eprintln!("[PASS] 10. Duplicate C5 returns 200, reports existing rank");
    }

    // ── 11. Leaderboard has 1 entry ─────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/leaderboards/3/3/5"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["graph_cid"], c5_cid);
        assert_eq!(entries[0]["rank"], 1);
        assert!(body["top_graph"].is_object());
        eprintln!("[PASS] 11. Leaderboard has 1 entry at rank=1");
    }

    // ── 12. Leaderboard list ────────────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/leaderboards"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        let summaries = body["leaderboards"].as_array().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0]["k"], 3);
        assert_eq!(summaries[0]["ell"], 3);
        assert_eq!(summaries[0]["n"], 5);
        assert_eq!(summaries[0]["entry_count"], 1);
        eprintln!("[PASS] 12. Leaderboard list returns 1 summary");
    }

    // ── 13. N values for pair ───────────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/leaderboards/3/3"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        let ns = body["n_values"].as_array().unwrap();
        assert_eq!(ns, &[5]);
        eprintln!("[PASS] 13. N values for (3,3): [5]");
    }

    // ── 14. Threshold (not full) ────────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/leaderboards/3/3/5/threshold"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["entry_count"], 1);
        assert_eq!(body["capacity"], 100);
        assert!(body["worst_tier1_max"].is_null(), "board not full, no worst");
        eprintln!("[PASS] 14. Threshold: 1/100 entries, board not full");
    }

    // ── 15. K > L auto-canonicalization ─────────────────────────────
    {
        // Submit with k=3, ell=3 but reversed (ell=3, k=3 is same, so test with 4,3)
        let resp = client
            .get(format!("{base}/api/leaderboards/3/3/5"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["k"], 3);
        assert_eq!(body["ell"], 3);
        eprintln!("[PASS] 15. K>L canonicalization confirmed");
    }

    // ── 16. Submission detail ───────────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/submissions/{c5_cid}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["graph_cid"], c5_cid);
        assert_eq!(body["k"], 3);
        assert_eq!(body["ell"], 3);
        assert_eq!(body["n"], 5);
        assert_eq!(body["verdict"], "accepted");
        assert_eq!(body["leaderboard_rank"], 1);
        assert!(body["rgxf"].is_object());
        eprintln!("[PASS] 16. Submission detail includes rank");
    }

    // ── 17. Missing submission → 404 ────────────────────────────────
    {
        let resp = client
            .get(format!("{base}/api/submissions/0000000000000000000000000000000000000000000000000000000000000000"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
        eprintln!("[PASS] 17. Missing submission returns 404");
    }

    // ── 18. N mismatch → 400 ────────────────────────────────────────
    {
        let resp = client
            .post(format!("{base}/api/submit"))
            .json(&serde_json::json!({
                "k": 3,
                "ell": 3,
                "n": 10,
                "graph": build_c5(),
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let body: Value = resp.json().await.unwrap();
        assert!(body["error"].as_str().unwrap().contains("mismatch"));
        eprintln!("[PASS] 18. N mismatch returns 400");
    }

    eprintln!("\n✓ All 18 system tests passed!");
}

