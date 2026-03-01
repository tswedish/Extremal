//! 3-tier lexicographic graph scoring for discovery ranking.
//!
//! **Tier 1** — Clique/independence structure (lowest wins):
//!   `(max(omega, alpha), min(omega, alpha))` lexicographic
//!
//! **Tier 2** — Maximum clique counts (lowest wins, tiebreaker):
//!   `(max(C_omega, C_alpha), min(C_omega, C_alpha))` lexicographic
//!
//! **Tier 3** — Automorphism group order (highest wins, tiebreaker):
//!   `|Aut(G)|` — rewards symmetric graphs

use std::cmp::Ordering;

use ramseynet_graph::AdjacencyMatrix;
use serde::Serialize;

use crate::automorphism::automorphism_group_order;
use crate::clique::count_max_cliques;

/// Full score for a discovered graph.
#[derive(Clone, Debug, Serialize)]
pub struct GraphScore {
    /// Clique number omega(G): max clique size.
    pub omega: u32,
    /// Independence number alpha(G): max independent set size.
    pub alpha: u32,
    /// Number of maximum cliques in G.
    pub c_omega: u64,
    /// Number of maximum independent sets (max cliques in complement).
    pub c_alpha: u64,
    /// Automorphism group order |Aut(G)|.
    pub aut_order: f64,
    // Pre-computed for fast comparison:
    tier1: (u32, u32), // (max, min) of (omega, alpha)
    tier2: (u64, u64), // (max, min) of (c_omega, c_alpha)
}

impl GraphScore {
    pub fn new(omega: u32, alpha: u32, c_omega: u64, c_alpha: u64, aut_order: f64) -> Self {
        let tier1 = (omega.max(alpha), omega.min(alpha));
        let tier2 = (c_omega.max(c_alpha), c_omega.min(c_alpha));
        Self {
            omega,
            alpha,
            c_omega,
            c_alpha,
            aut_order,
            tier1,
            tier2,
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
        // T1: lower is better (ascending)
        self.tier1
            .cmp(&other.tier1)
            // T2: lower is better (ascending)
            .then(self.tier2.cmp(&other.tier2))
            // T3: higher is better (descending) — reverse comparison
            .then(other.aut_order.total_cmp(&self.aut_order))
    }
}

/// Compute the full 3-tier score for a graph.
///
/// Computes clique/independence structure on G and complement, plus
/// automorphism group order via nauty.
pub fn compute_score(graph: &AdjacencyMatrix) -> GraphScore {
    let (omega, c_omega) = count_max_cliques(graph);
    let comp = graph.complement();
    let (alpha, c_alpha) = count_max_cliques(&comp);
    let aut_order = automorphism_group_order(graph);

    GraphScore::new(omega, alpha, c_omega, c_alpha, aut_order)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn c5_score() {
        let score = compute_score(&make_c5());
        assert_eq!(score.omega, 2);
        assert_eq!(score.alpha, 2); // complement of C5 is also C5
        assert_eq!(score.c_omega, 5);
        assert_eq!(score.c_alpha, 5);
        assert_eq!(score.aut_order, 10.0);
    }

    #[test]
    fn k5_score() {
        let score = compute_score(&make_k5());
        assert_eq!(score.omega, 5);
        assert_eq!(score.alpha, 1);
        assert_eq!(score.c_omega, 1);
        assert_eq!(score.c_alpha, 5);
        assert_eq!(score.aut_order, 120.0);
    }

    /// Lower tier1 wins regardless of other tiers.
    #[test]
    fn tier1_dominates() {
        let better = GraphScore::new(2, 2, 100, 100, 1.0);
        let worse = GraphScore::new(3, 2, 1, 1, 1000.0);
        assert!(better < worse);
    }

    /// Same tier1, lower tier2 wins.
    #[test]
    fn tier2_breaks_tie() {
        let better = GraphScore::new(2, 2, 3, 3, 1.0);
        let worse = GraphScore::new(2, 2, 5, 5, 1000.0);
        assert!(better < worse);
    }

    /// Same tier1 and tier2, higher aut_order wins (lower in Ord).
    #[test]
    fn tier3_breaks_tie() {
        let better = GraphScore::new(2, 2, 5, 5, 100.0);
        let worse = GraphScore::new(2, 2, 5, 5, 10.0);
        assert!(better < worse);
    }

    /// Symmetry: (omega, alpha) and (alpha, omega) produce the same tier1.
    #[test]
    fn tier1_symmetry() {
        let a = GraphScore::new(2, 3, 5, 10, 10.0);
        let b = GraphScore::new(3, 2, 10, 5, 10.0);
        assert_eq!(a.tier1, b.tier1);
        assert_eq!(a.tier2, b.tier2);
        assert_eq!(a.cmp(&b), Ordering::Equal);
    }
}
