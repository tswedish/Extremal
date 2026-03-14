//! 3-tier lexicographic graph scoring for discovery ranking.
//!
//! **Tier 1** — Maximum clique counts (lowest wins):
//!   `(max(C_omega, C_alpha), min(C_omega, C_alpha))` lexicographic
//!
//! **Tier 2** — Automorphism group order (highest wins):
//!   `|Aut(G)|` — rewards symmetric graphs
//!
//! **Tier 3** — CID tiebreaker (smallest wins):
//!   Deterministic byte-level comparison

use std::cmp::Ordering;

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_types::GraphCid;
use serde::{Deserialize, Serialize};

use crate::automorphism::canonical_form;
use crate::clique::count_max_cliques;

/// Full score for a discovered graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphScore {
    /// Clique number omega(G): max clique size (display only, not in Ord).
    pub omega: u32,
    /// Independence number alpha(G): max independent set size (display only, not in Ord).
    pub alpha: u32,
    /// Number of maximum cliques in G.
    pub c_omega: u64,
    /// Number of maximum independent sets (max cliques in complement).
    pub c_alpha: u64,
    /// Automorphism group order |Aut(G)|.
    pub aut_order: f64,
    /// Content ID of the graph (deterministic tiebreaker).
    pub cid: GraphCid,
    // Pre-computed for fast comparison:
    tier1: (u64, u64), // (max, min) of (c_omega, c_alpha)
}

impl GraphScore {
    pub fn new(
        omega: u32,
        alpha: u32,
        c_omega: u64,
        c_alpha: u64,
        aut_order: f64,
        cid: GraphCid,
    ) -> Self {
        let tier1 = (c_omega.max(c_alpha), c_omega.min(c_alpha));
        Self {
            omega,
            alpha,
            c_omega,
            c_alpha,
            aut_order,
            cid,
            tier1,
        }
    }
}

impl PartialEq for GraphScore {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for GraphScore {}

impl PartialOrd for GraphScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GraphScore {
    fn cmp(&self, other: &Self) -> Ordering {
        // T1: lower clique counts win (ascending)
        self.tier1
            .cmp(&other.tier1)
            // T2: higher aut wins (descending)
            .then(other.aut_order.total_cmp(&self.aut_order))
            // T3: smaller CID wins (ascending)
            .then(self.cid.cmp(&other.cid))
    }
}

/// Result of scoring a graph, including its canonical form.
pub struct ScoreResult {
    /// The 3-tier score.
    pub score: GraphScore,
    /// The graph in canonical form (nauty canonical labeling applied).
    pub canonical_graph: AdjacencyMatrix,
}

/// Compute the full 3-tier score for a graph, producing its canonical form.
///
/// Computes clique/independence structure on G and complement, plus
/// automorphism group order and canonical labeling via nauty (single call).
/// The CID used in the score is computed from the **canonical** form.
pub fn compute_score_canonical(graph: &AdjacencyMatrix) -> ScoreResult {
    let (omega, c_omega) = count_max_cliques(graph);
    let comp = graph.complement();
    let (alpha, c_alpha) = count_max_cliques(&comp);
    let (canonical_graph, aut_order) = canonical_form(graph);

    let canonical_cid = ramseynet_graph::compute_cid(&canonical_graph);
    let score = GraphScore::new(omega, alpha, c_omega, c_alpha, aut_order, canonical_cid);

    ScoreResult {
        score,
        canonical_graph,
    }
}

/// Compute the full 3-tier score for a graph (legacy: uses provided CID).
///
/// Prefer `compute_score_canonical` which derives the CID from the canonical form.
pub fn compute_score(graph: &AdjacencyMatrix, cid: &GraphCid) -> GraphScore {
    let (omega, c_omega) = count_max_cliques(graph);
    let comp = graph.complement();
    let (alpha, c_alpha) = count_max_cliques(&comp);
    let (_, aut_order) = canonical_form(graph);

    GraphScore::new(omega, alpha, c_omega, c_alpha, aut_order, cid.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ramseynet_graph::compute_cid;

    fn make_c5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        g
    }

    fn make_k5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        g
    }

    /// Dummy CID for testing (not a real hash).
    fn test_cid(byte: u8) -> GraphCid {
        GraphCid([byte; 32])
    }

    #[test]
    fn c5_score() {
        let g = make_c5();
        let cid = compute_cid(&g);
        let score = compute_score(&g, &cid);
        assert_eq!(score.omega, 2);
        assert_eq!(score.alpha, 2); // complement of C5 is also C5
        assert_eq!(score.c_omega, 5);
        assert_eq!(score.c_alpha, 5);
        assert_eq!(score.aut_order, 10.0);
    }

    #[test]
    fn k5_score() {
        let g = make_k5();
        let cid = compute_cid(&g);
        let score = compute_score(&g, &cid);
        assert_eq!(score.omega, 5);
        assert_eq!(score.alpha, 1);
        assert_eq!(score.c_omega, 1);
        assert_eq!(score.c_alpha, 5);
        assert_eq!(score.aut_order, 120.0);
    }

    /// Lower tier1 wins regardless of other tiers.
    #[test]
    fn tier1_dominates() {
        let better = GraphScore::new(2, 2, 3, 3, 1.0, test_cid(0xff));
        let worse = GraphScore::new(3, 2, 5, 5, 1000.0, test_cid(0x00));
        assert!(better < worse);
    }

    /// Same tier1, higher aut_order wins (lower in Ord).
    #[test]
    fn tier2_breaks_tie() {
        let better = GraphScore::new(2, 2, 5, 5, 100.0, test_cid(0xff));
        let worse = GraphScore::new(2, 2, 5, 5, 10.0, test_cid(0x00));
        assert!(better < worse);
    }

    /// Same tier1 and tier2, smaller CID wins.
    #[test]
    fn tier3_breaks_tie() {
        let better = GraphScore::new(2, 2, 5, 5, 10.0, test_cid(0x00));
        let worse = GraphScore::new(2, 2, 5, 5, 10.0, test_cid(0xff));
        assert!(better < worse);
    }

    /// Symmetry: (c_omega, c_alpha) and (c_alpha, c_omega) produce the same tier1.
    #[test]
    fn tier1_symmetry() {
        let cid = test_cid(0x42);
        let a = GraphScore::new(2, 3, 5, 10, 10.0, cid.clone());
        let b = GraphScore::new(3, 2, 10, 5, 10.0, cid);
        assert_eq!(a.tier1, b.tier1);
        assert_eq!(a.cmp(&b), Ordering::Equal);
    }
}
