use ramseynet_types::GraphCid;
use sha2::{Digest, Sha256};

use crate::adjacency::AdjacencyMatrix;
use crate::rgxf::to_canonical_bytes;

/// Compute the content ID (SHA-256 of canonical RGXF bytes) for a graph.
pub fn compute_cid(matrix: &AdjacencyMatrix) -> GraphCid {
    let canonical = to_canonical_bytes(matrix);
    let hash = Sha256::digest(&canonical);
    GraphCid(hash.into())
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
}
