//! 4-tier lexicographic graph scoring for discovery ranking.
//!
//! **Tier 1** — Maximum clique counts (lowest wins):
//!   `(max(C_omega, C_alpha), min(C_omega, C_alpha))` lexicographic
//!
//! **Tier 2** — Goodman gap (lowest wins):
//!   Distance from Goodman's minimum monochromatic triangle count.
//!   A gap of 0 means the graph achieves the theoretical minimum.
//!
//! **Tier 3** — Automorphism group order (highest wins):
//!   `|Aut(G)|` — rewards symmetric graphs
//!
//! **Tier 4** — CID tiebreaker (smallest wins):
//!   Deterministic byte-level comparison

use std::cmp::Ordering;

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_types::GraphCid;
use serde::{Deserialize, Serialize};

use crate::automorphism::canonical_form;
use crate::clique::{count_cliques, count_max_cliques};

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
    /// Number of triangles (3-cliques) in G.
    #[serde(default)]
    pub triangles: u64,
    /// Number of triangles in the complement (independent 3-sets in G).
    #[serde(default)]
    pub triangles_complement: u64,
    /// Goodman number: total monochromatic triangles = triangles + triangles_complement.
    #[serde(default)]
    pub goodman: u64,
    /// Goodman gap: distance from Goodman's minimum for this n. 0 = optimal.
    #[serde(default)]
    pub goodman_gap: u64,
    /// Goodman's theoretical minimum for this n. Included so consumers
    /// don't need to reimplement the formula.
    #[serde(default)]
    pub goodman_min: u64,
    /// Automorphism group order |Aut(G)|.
    pub aut_order: f64,
    /// Content ID of the graph (deterministic tiebreaker).
    pub cid: GraphCid,
    // Pre-computed for fast comparison:
    tier1: (u64, u64), // (max, min) of (c_omega, c_alpha)
}

impl GraphScore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        n: u32,
        omega: u32,
        alpha: u32,
        c_omega: u64,
        c_alpha: u64,
        triangles: u64,
        triangles_complement: u64,
        aut_order: f64,
        cid: GraphCid,
    ) -> Self {
        let tier1 = (c_omega.max(c_alpha), c_omega.min(c_alpha));
        let goodman = triangles + triangles_complement;
        let goodman_min = goodman_minimum(n);
        let goodman_gap = goodman.saturating_sub(goodman_min);
        Self {
            omega,
            alpha,
            c_omega,
            c_alpha,
            triangles,
            triangles_complement,
            goodman,
            goodman_gap,
            goodman_min,
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
            // T2: lower Goodman gap wins (ascending) — 0 is optimal
            .then(self.goodman_gap.cmp(&other.goodman_gap))
            // T3: higher aut wins (descending)
            .then(other.aut_order.total_cmp(&self.aut_order))
            // T4: smaller CID wins (ascending)
            .then(self.cid.cmp(&other.cid))
    }
}

/// Result of scoring a graph, including its canonical form.
pub struct ScoreResult {
    /// The 4-tier score.
    pub score: GraphScore,
    /// The graph in canonical form (nauty canonical labeling applied).
    pub canonical_graph: AdjacencyMatrix,
}

/// Compute the full 4-tier score for a graph, producing its canonical form.
///
/// Computes clique/independence structure on G and complement, triangle
/// counts for the Goodman number, automorphism group order and canonical
/// labeling via nauty (single call). The CID used in the score is computed
/// from the **canonical** form.
pub fn compute_score_canonical(graph: &AdjacencyMatrix) -> ScoreResult {
    let (omega, c_omega) = count_max_cliques(graph);
    let comp = graph.complement();
    let (alpha, c_alpha) = count_max_cliques(&comp);
    let triangles = count_cliques(graph, 3);
    let triangles_complement = count_cliques(&comp, 3);
    let (canonical_graph, aut_order) = canonical_form(graph);

    let canonical_cid = ramseynet_graph::compute_cid(&canonical_graph);
    let score = GraphScore::new(
        graph.n(),
        omega,
        alpha,
        c_omega,
        c_alpha,
        triangles,
        triangles_complement,
        aut_order,
        canonical_cid,
    );

    ScoreResult {
        score,
        canonical_graph,
    }
}

/// Compute the full 4-tier score for a graph (legacy: uses provided CID).
///
/// Prefer `compute_score_canonical` which derives the CID from the canonical form.
pub fn compute_score(graph: &AdjacencyMatrix, cid: &GraphCid) -> GraphScore {
    let (omega, c_omega) = count_max_cliques(graph);
    let comp = graph.complement();
    let (alpha, c_alpha) = count_max_cliques(&comp);
    let triangles = count_cliques(graph, 3);
    let triangles_complement = count_cliques(&comp, 3);
    let (_, aut_order) = canonical_form(graph);

    GraphScore::new(
        graph.n(),
        omega,
        alpha,
        c_omega,
        c_alpha,
        triangles,
        triangles_complement,
        aut_order,
        cid.clone(),
    )
}

/// Compute Goodman's minimum: the minimum total number of monochromatic
/// triangles in any 2-coloring of K_n.
///
/// Formula: g(n) = C(n,3) - floor(n * floor((n-1)^2 / 4) / 2)
///
/// Equivalently: achieved when all vertex degrees equal floor((n-1)/2).
/// Cross-validated against the degree-sum reference in tests.
pub fn goodman_minimum(n: u32) -> u64 {
    if n < 3 {
        return 0;
    }
    let n = n as u64;
    let c_n_3 = n * (n - 1) * (n - 2) / 6;
    // Minimum is achieved when all vertex degrees equal floor((n-1)/2).
    // floor_term = floor(n * floor((n-1)^2 / 4) / 2)
    let floor_term = n * ((n - 1) * (n - 1) / 4) / 2;
    c_n_3 - floor_term
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
        // C5 is triangle-free, and its complement (also C5) is also triangle-free
        assert_eq!(score.triangles, 0);
        assert_eq!(score.triangles_complement, 0);
        assert_eq!(score.goodman, 0);
        // Goodman minimum for n=5: C(5,3) - floor(5/2)*floor(16/4) = 10 - 2*4 = 2
        // But C5 has goodman=0 which is below the minimum? Let's check...
        // Actually g(5) = 10 - 2*4 = 2. But C5 has 0 triangles in both colorings.
        // This is possible because Goodman's formula applies to COMPLETE graphs,
        // and C5 with 5 edges is far from complete. For Ramsey graphs specifically,
        // the Goodman number counts only triangles, not all monochromatic subgraphs.
        // So gap = max(0, 0 - 2) = 0 (saturating_sub).
        assert_eq!(score.goodman_gap, 0);
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
        // K5 has C(5,3) = 10 triangles, complement (E5) has 0
        assert_eq!(score.triangles, 10);
        assert_eq!(score.triangles_complement, 0);
        assert_eq!(score.goodman, 10);
        // g(5) = 0, gap = 10 - 0 = 10
        assert_eq!(score.goodman_gap, 10);
    }

    /// Lower tier1 wins regardless of other tiers.
    #[test]
    fn tier1_dominates() {
        let better = GraphScore::new(5, 2, 2, 3, 3, 0, 0, 1.0, test_cid(0xff));
        let worse = GraphScore::new(5, 3, 2, 5, 5, 0, 0, 1000.0, test_cid(0x00));
        assert!(better < worse);
    }

    /// Same tier1, lower Goodman gap wins.
    #[test]
    fn tier2_goodman_gap_breaks_tie() {
        // Same clique counts, different Goodman gaps
        let better = GraphScore::new(5, 2, 2, 5, 5, 0, 0, 10.0, test_cid(0xff));
        let worse = GraphScore::new(5, 2, 2, 5, 5, 5, 5, 10.0, test_cid(0x00));
        // better has goodman=0, gap=0; worse has goodman=10, gap=8
        assert!(better < worse);
    }

    /// Same tier1 and Goodman gap, higher aut_order wins (lower in Ord).
    #[test]
    fn tier3_aut_breaks_tie() {
        let better = GraphScore::new(5, 2, 2, 5, 5, 1, 1, 100.0, test_cid(0xff));
        let worse = GraphScore::new(5, 2, 2, 5, 5, 1, 1, 10.0, test_cid(0x00));
        assert!(better < worse);
    }

    /// Same tier1, tier2, tier3, smaller CID wins.
    #[test]
    fn tier4_cid_breaks_tie() {
        let better = GraphScore::new(5, 2, 2, 5, 5, 1, 1, 10.0, test_cid(0x00));
        let worse = GraphScore::new(5, 2, 2, 5, 5, 1, 1, 10.0, test_cid(0xff));
        assert!(better < worse);
    }

    /// Symmetry: (c_omega, c_alpha) and (c_alpha, c_omega) produce the same tier1.
    #[test]
    fn tier1_symmetry() {
        let cid = test_cid(0x42);
        let a = GraphScore::new(5, 2, 3, 5, 10, 0, 0, 10.0, cid.clone());
        let b = GraphScore::new(5, 3, 2, 10, 5, 0, 0, 10.0, cid);
        assert_eq!(a.tier1, b.tier1);
        assert_eq!(a.cmp(&b), Ordering::Equal);
    }

    /// Goodman minimum for small values of n.
    /// Compute the exact Goodman minimum via the degree-sum formulation.
    /// This is the "reference" implementation used only for testing.
    ///
    /// g(n) = C(n,3) - sum(d_v * (n-1-d_v)) / 2
    /// minimized when all d_v = floor((n-1)/2) or ceil((n-1)/2).
    fn goodman_minimum_exact(n: u32) -> u64 {
        if n < 3 {
            return 0;
        }
        let n = n as u64;
        let c_n_3 = n * (n - 1) * (n - 2) / 6;
        let d_low = (n - 1) / 2;
        let d_high = n / 2; // = ceil((n-1)/2)
        let sum = if n % 2 == 1 {
            // All degrees = (n-1)/2 (exact integer)
            n * d_low * (n - 1 - d_low)
        } else {
            // n/2 vertices at d_low, n/2 at d_high
            (n / 2) * d_low * (n - 1 - d_low) + (n / 2) * d_high * (n - 1 - d_high)
        };
        c_n_3 - sum / 2
    }

    /// Cross-validate goodman_minimum() against the exact degree-sum
    /// reference for n = 0..50. This ensures the closed-form integer
    /// formula matches the definitional computation.
    #[test]
    fn goodman_minimum_cross_validation() {
        for n in 0..50 {
            let fast = goodman_minimum(n);
            let exact = goodman_minimum_exact(n);
            assert_eq!(
                fast, exact,
                "goodman_minimum({n}) = {fast}, expected {exact} (from degree-sum)"
            );
        }
    }

    /// Spot-check specific known values.
    #[test]
    fn goodman_minimum_known_values() {
        assert_eq!(goodman_minimum(0), 0);
        assert_eq!(goodman_minimum(1), 0);
        assert_eq!(goodman_minimum(2), 0);
        assert_eq!(goodman_minimum(3), 0);
        assert_eq!(goodman_minimum(4), 0);
        assert_eq!(goodman_minimum(5), 0);
        assert_eq!(goodman_minimum(6), 2);
        assert_eq!(goodman_minimum(7), 4);
        assert_eq!(goodman_minimum(8), 8);
        assert_eq!(goodman_minimum(9), 12);
        assert_eq!(goodman_minimum(17), 136);
        assert_eq!(goodman_minimum(25), 500);
    }
}
