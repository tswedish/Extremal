pub mod automorphism;
pub mod clique;
pub mod scoring;
pub mod types;

pub use automorphism::canonical_form;

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_types::{GraphCid, Verdict};

pub use types::{VerifyRequest, VerifyResponse, VerifyResult};

/// Core Ramsey verification: check that omega(G) < k AND alpha(G) < ell.
///
/// Returns `Accepted` if the graph has no k-clique and no ell-independent-set.
/// Returns `Rejected` with a canonical witness (lexicographically smallest violating set).
pub fn verify_ramsey(adj: &AdjacencyMatrix, k: u32, ell: u32, cid: &GraphCid) -> VerifyResult {
    // Check for k-clique in G
    if let Some(witness) = clique::find_clique_witness(adj, k) {
        return VerifyResult {
            verdict: Verdict::Rejected,
            graph_cid: cid.clone(),
            reason: Some("clique_found".into()),
            witness: Some(witness),
        };
    }

    // Check for ell-independent-set = ell-clique in complement
    let comp = adj.complement();
    if let Some(witness) = clique::find_clique_witness(&comp, ell) {
        return VerifyResult {
            verdict: Verdict::Rejected,
            graph_cid: cid.clone(),
            reason: Some("independent_set_found".into()),
            witness: Some(witness),
        };
    }

    VerifyResult {
        verdict: Verdict::Accepted,
        graph_cid: cid.clone(),
        reason: None,
        witness: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramseynet_graph::compute_cid;

    /// C5 (5-cycle) should be accepted for R(3,3): omega=2, alpha=2.
    #[test]
    fn c5_accepted_for_r33() {
        let mut g = AdjacencyMatrix::new(5);
        // 5-cycle: 0-1-2-3-4-0
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(2, 3, true);
        g.set_edge(3, 4, true);
        g.set_edge(4, 0, true);

        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 3, &cid);
        assert_eq!(result.verdict, Verdict::Accepted);
        assert!(result.witness.is_none());
    }

    /// K5 (complete graph on 5 vertices) should be rejected for R(3,3): has 3-clique.
    #[test]
    fn k5_rejected_for_r33_clique() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 3, &cid);
        assert_eq!(result.verdict, Verdict::Rejected);
        assert_eq!(result.reason.as_deref(), Some("clique_found"));
        let witness = result.witness.unwrap();
        assert_eq!(witness.len(), 3);
        // Witness should be lex-smallest: [0, 1, 2]
        assert_eq!(witness, vec![0, 1, 2]);
    }

    /// Empty graph on 5 vertices should be rejected for R(3,3): has 3-independent-set.
    #[test]
    fn empty_5_rejected_for_r33_independent_set() {
        let g = AdjacencyMatrix::new(5);
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 3, &cid);
        assert_eq!(result.verdict, Verdict::Rejected);
        assert_eq!(result.reason.as_deref(), Some("independent_set_found"));
        let witness = result.witness.unwrap();
        assert_eq!(witness.len(), 3);
        assert_eq!(witness, vec![0, 1, 2]);
    }

    /// Wagner graph (n=8, 3-regular, circulant C(8,{1,4})) should be accepted for R(3,4).
    /// Triangle-free (omega=2) and alpha=3, so omega < 3 and alpha < 4.
    #[test]
    fn wagner_accepted_for_r34() {
        let mut g = AdjacencyMatrix::new(8);
        // Circulant C(8, {1, 4}): each vertex i connects to (i±1)%8 and (i+4)%8
        for i in 0..8 {
            g.set_edge(i, (i + 1) % 8, true);
            g.set_edge(i, (i + 4) % 8, true);
        }
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 4, &cid);
        assert_eq!(
            result.verdict,
            Verdict::Accepted,
            "Wagner graph should be accepted for R(3,4): {:?}",
            result
        );
        assert!(result.witness.is_none());
    }

    /// Wagner graph RGXF round-trip: verify the base64 encoding matches.
    #[test]
    fn wagner_rgxf_encoding() {
        use ramseynet_graph::rgxf;

        let mut g = AdjacencyMatrix::new(8);
        for i in 0..8 {
            g.set_edge(i, (i + 1) % 8, true);
            g.set_edge(i, (i + 4) % 8, true);
        }
        let json = rgxf::to_json(&g);
        assert_eq!(json.bits_b64, "kySmUA==");
        assert_eq!(json.n, 8);

        // Round-trip decode
        let decoded = rgxf::from_json(&json).unwrap();
        assert_eq!(decoded.n(), 8);
        for i in 0..8 {
            assert!(
                decoded.edge(i, (i + 1) % 8),
                "missing edge {}-{}",
                i,
                (i + 1) % 8
            );
            assert!(
                decoded.edge(i, (i + 4) % 8),
                "missing edge {}-{}",
                i,
                (i + 4) % 8
            );
        }
    }

    /// Petersen graph (n=10) should be REJECTED for R(3,4): alpha=4 which is not < 4.
    #[test]
    fn petersen_rejected_for_r34() {
        let mut g = AdjacencyMatrix::new(10);
        // Outer cycle
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        // Inner pentagram
        for i in 0..5 {
            g.set_edge(5 + i, 5 + (i + 2) % 5, true);
        }
        // Spokes
        for i in 0..5 {
            g.set_edge(i, i + 5, true);
        }

        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 4, &cid);
        assert_eq!(result.verdict, Verdict::Rejected);
        assert_eq!(result.reason.as_deref(), Some("independent_set_found"));
        let witness = result.witness.unwrap();
        assert_eq!(
            witness.len(),
            4,
            "Petersen graph has alpha=4, witness should be size 4"
        );
    }
}
