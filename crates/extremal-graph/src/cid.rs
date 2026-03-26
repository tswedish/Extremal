//! Content addressing for graphs via blake3 hashing.
//!
//! The CID (Content IDentifier) for a graph is `blake3(graph6_bytes)` where
//! `graph6_bytes` is the graph6 encoding of the **canonical** form of the
//! graph.
//!
//! Note: This module computes the hash of whatever graph is passed in.
//! Canonical labeling (via nauty) must be applied *before* calling
//! `compute_cid` to ensure isomorphic graphs get the same CID. The
//! `extremal-scoring` crate handles canonicalization.

use extremal_types::GraphCid;

use crate::adjacency::AdjacencyMatrix;
use crate::graph6;

/// Compute the content identifier for a graph.
///
/// Returns `blake3(graph6_encode(matrix))`.
///
/// **Important**: For deduplication, the matrix should be in canonical form
/// (nauty-relabeled) before calling this function.
pub fn compute_cid(matrix: &AdjacencyMatrix) -> GraphCid {
    let g6 = graph6::encode(matrix);
    let hash = blake3::hash(g6.as_bytes());
    GraphCid::from_bytes(*hash.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_deterministic() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(2, 3, true);
        let cid1 = compute_cid(&g);
        let cid2 = compute_cid(&g);
        assert_eq!(cid1, cid2);
    }

    #[test]
    fn different_graphs_different_cids() {
        let mut g1 = AdjacencyMatrix::new(5);
        g1.set_edge(0, 1, true);

        let mut g2 = AdjacencyMatrix::new(5);
        g2.set_edge(0, 2, true);

        assert_ne!(compute_cid(&g1), compute_cid(&g2));
    }

    #[test]
    fn cid_is_blake3() {
        let g = AdjacencyMatrix::new(5);
        let g6 = graph6::encode(&g);
        let expected = blake3::hash(g6.as_bytes());
        let cid = compute_cid(&g);
        assert_eq!(cid.as_bytes(), expected.as_bytes());
    }
}
