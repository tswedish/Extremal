//! 2-color k-clique histogram computation.
//!
//! For a graph G on n vertices, we consider the 2-coloring where "red" is G
//! and "blue" is the complement of G. For each k from 3 up to some max_k,
//! we count the number of k-cliques in red (G) and blue (complement).
//!
//! The histogram is the foundation of the MineGraph scoring system.

use minegraph_graph::AdjacencyMatrix;
use serde::{Deserialize, Serialize};

use crate::clique::{NeighborSet, count_cliques};

/// A single tier in the clique histogram: counts for one value of k.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistogramTier {
    /// The clique size k.
    pub k: u32,
    /// Number of k-cliques in the graph (red).
    pub red: u64,
    /// Number of k-cliques in the complement (blue).
    pub blue: u64,
}

/// The full 2-color k-clique histogram for a graph.
///
/// Contains tiers for k = 3, 4, 5, ... up to the largest k where at least
/// one color has nonzero count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliqueHistogram {
    /// Tiers from k=3 upward. Empty tiers at the top are trimmed.
    pub tiers: Vec<HistogramTier>,
    /// Vertex count of the graph.
    pub n: u32,
}

impl CliqueHistogram {
    /// Compute the histogram for a graph.
    ///
    /// `max_k` is the maximum clique size to check. For R(5,5) search at
    /// n=25, use max_k=5 or higher. Setting max_k too high wastes time
    /// on zero counts but is not incorrect.
    pub fn compute(matrix: &AdjacencyMatrix, max_k: u32) -> Self {
        let n = matrix.n();
        let adj_nbrs = NeighborSet::from_adj(matrix);
        let comp = matrix.complement();
        let comp_nbrs = NeighborSet::from_adj(&comp);

        let mut tiers = Vec::new();
        for k in 3..=max_k {
            let red = count_cliques(&adj_nbrs, k, n);
            let blue = count_cliques(&comp_nbrs, k, n);
            tiers.push(HistogramTier { k, red, blue });
        }

        // Trim trailing zero tiers (where both red and blue are 0)
        while tiers.last().is_some_and(|t| t.red == 0 && t.blue == 0) {
            tiers.pop();
        }

        Self { tiers, n }
    }

    /// The maximum k with nonzero clique count.
    pub fn max_k(&self) -> Option<u32> {
        self.tiers.last().map(|t| t.k)
    }

    /// Whether the graph has zero violations for a given (k, ell) target.
    ///
    /// A graph is valid for R(k, ell) if it has no k-cliques in red and
    /// no ell-cliques in blue (complement).
    pub fn is_valid_ramsey(&self, k: u32, ell: u32) -> bool {
        for tier in &self.tiers {
            if tier.k == k && tier.red > 0 {
                return false;
            }
            if tier.k == ell && tier.blue > 0 {
                return false;
            }
        }
        true
    }

    /// Get the tier for a specific k, or None if k < 3 or not computed.
    pub fn tier(&self, k: u32) -> Option<&HistogramTier> {
        self.tiers.iter().find(|t| t.k == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_k5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5u32 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        g
    }

    fn make_c5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(2, 3, true);
        g.set_edge(3, 4, true);
        g.set_edge(0, 4, true);
        g
    }

    #[test]
    fn k5_histogram() {
        let g = make_k5();
        let hist = CliqueHistogram::compute(&g, 5);
        // K5 red: C(5,3)=10 triangles, C(5,4)=5 4-cliques, C(5,5)=1 5-clique
        // K5 complement is empty: 0 blue cliques
        assert_eq!(hist.tiers.len(), 3); // k=3,4,5
        assert_eq!(hist.tier(3).unwrap().red, 10);
        assert_eq!(hist.tier(3).unwrap().blue, 0);
        assert_eq!(hist.tier(4).unwrap().red, 5);
        assert_eq!(hist.tier(5).unwrap().red, 1);
        assert!(!hist.is_valid_ramsey(5, 5)); // has 5-clique in red
    }

    #[test]
    fn c5_histogram() {
        let g = make_c5();
        let hist = CliqueHistogram::compute(&g, 5);
        // C5 has 0 triangles. Complement of C5 is C5 (self-complementary).
        // So both red and blue have 0 triangles.
        assert!(hist.tiers.is_empty() || hist.tier(3).unwrap().red == 0);
    }

    #[test]
    fn empty_graph_histogram() {
        let g = AdjacencyMatrix::new(5);
        let hist = CliqueHistogram::compute(&g, 5);
        // Empty graph: no red cliques. Complement is K5: has all cliques.
        assert_eq!(hist.tier(3).unwrap().red, 0);
        assert_eq!(hist.tier(3).unwrap().blue, 10);
        assert_eq!(hist.tier(5).unwrap().blue, 1);
    }

    #[test]
    fn valid_ramsey_check() {
        // C5 is self-complementary, triangle-free: valid for R(3,3)
        let g = make_c5();
        let hist = CliqueHistogram::compute(&g, 5);
        assert!(hist.is_valid_ramsey(3, 3));
        // But K5 is not valid for R(5,5)
        let g2 = make_k5();
        let hist2 = CliqueHistogram::compute(&g2, 5);
        assert!(!hist2.is_valid_ramsey(5, 5));
    }
}
