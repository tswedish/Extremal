//! Packed upper-triangular adjacency matrix for undirected simple graphs.
//!
//! Ported from the RamseyNet prototype with the same proven bit-packing layout.

use serde::{Deserialize, Serialize};

/// Packed upper-triangular adjacency matrix for an undirected graph.
///
/// For n vertices, stores n*(n-1)/2 bits packed into bytes (MSB-first within
/// each byte). Bit index for edge (i,j) where i < j:
/// `i*n - i*(i+1)/2 + (j - i - 1)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjacencyMatrix {
    n: u32,
    bits: Vec<u8>,
}

impl AdjacencyMatrix {
    /// Create a new graph with `n` vertices and no edges.
    pub fn new(n: u32) -> Self {
        let num_bits = Self::total_bits(n);
        let num_bytes = num_bits.div_ceil(8);
        Self {
            n,
            bits: vec![0u8; num_bytes],
        }
    }

    /// Create from pre-packed bits. Returns an error if the byte vector
    /// length doesn't match the vertex count.
    pub fn from_bits(n: u32, bits: Vec<u8>) -> Result<Self, &'static str> {
        let num_bits = Self::total_bits(n);
        let expected_bytes = num_bits.div_ceil(8);
        if bits.len() != expected_bytes {
            return Err("bit vector length does not match vertex count");
        }
        Ok(Self { n, bits })
    }

    /// Number of vertices.
    pub fn n(&self) -> u32 {
        self.n
    }

    /// Total number of possible edges: n*(n-1)/2.
    pub fn total_bits(n: u32) -> usize {
        let n = n as usize;
        if n < 2 {
            return 0;
        }
        n * (n - 1) / 2
    }

    /// Check if edge (i, j) exists. Order of i, j does not matter.
    pub fn edge(&self, i: u32, j: u32) -> bool {
        if i == j {
            return false;
        }
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        let idx = Self::bit_index(self.n, lo, hi);
        let byte_idx = idx / 8;
        let bit_idx = 7 - (idx % 8); // MSB-first
        (self.bits[byte_idx] >> bit_idx) & 1 == 1
    }

    /// Set or clear edge (i, j). Order of i, j does not matter.
    pub fn set_edge(&mut self, i: u32, j: u32, value: bool) {
        if i == j {
            return;
        }
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        let idx = Self::bit_index(self.n, lo, hi);
        let byte_idx = idx / 8;
        let bit_idx = 7 - (idx % 8);
        if value {
            self.bits[byte_idx] |= 1 << bit_idx;
        } else {
            self.bits[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Count the number of edges in the graph.
    pub fn num_edges(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// Get all neighbors of vertex v.
    pub fn neighbors(&self, v: u32) -> Vec<u32> {
        (0..self.n).filter(|&u| self.edge(v, u)).collect()
    }

    /// Degree of vertex v.
    pub fn degree(&self, v: u32) -> u32 {
        self.neighbors(v).len() as u32
    }

    /// Compute the complement graph (flip all edges).
    pub fn complement(&self) -> Self {
        let mut comp = Self::new(self.n);
        for i in 0..self.n {
            for j in (i + 1)..self.n {
                comp.set_edge(i, j, !self.edge(i, j));
            }
        }
        comp
    }

    /// Relabel vertices according to a permutation.
    ///
    /// `perm[i]` gives the new label for old vertex `i`.
    /// The returned graph has edge (perm[i], perm[j]) iff the original has
    /// edge (i, j).
    pub fn permute_vertices(&self, perm: &[u32]) -> Self {
        assert_eq!(
            perm.len(),
            self.n as usize,
            "permutation length must equal vertex count"
        );
        let mut result = Self::new(self.n);
        for i in 0..self.n {
            for j in (i + 1)..self.n {
                if self.edge(i, j) {
                    result.set_edge(perm[i as usize], perm[j as usize], true);
                }
            }
        }
        result
    }

    /// Get the packed bit vector (for serialization).
    pub fn packed_bits(&self) -> &[u8] {
        &self.bits
    }

    /// Build neighbor bitmask array. `masks[v]` has bit `w` set iff edge(v,w).
    ///
    /// Supports n up to 64. This enables bitwise-parallel clique counting:
    /// common neighbors = `masks[u] & masks[v]`, clique membership =
    /// AND + popcount.
    pub fn neighbor_masks(&self) -> Vec<u64> {
        assert!(self.n <= 64, "neighbor_masks requires n <= 64");
        let n = self.n;
        let mut masks = vec![0u64; n as usize];
        for i in 0..n {
            for j in (i + 1)..n {
                if self.edge(i, j) {
                    masks[i as usize] |= 1u64 << j;
                    masks[j as usize] |= 1u64 << i;
                }
            }
        }
        masks
    }

    /// Bit index within the packed upper-triangular representation.
    /// Requires i < j.
    fn bit_index(n: u32, i: u32, j: u32) -> usize {
        debug_assert!(i < j);
        let n = n as usize;
        let i = i as usize;
        let j = j as usize;
        i * n - i * (i + 1) / 2 + (j - i - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_graph_has_no_edges() {
        let g = AdjacencyMatrix::new(10);
        assert_eq!(g.num_edges(), 0);
        for i in 0..10 {
            for j in 0..10 {
                assert!(!g.edge(i, j));
            }
        }
    }

    #[test]
    fn set_and_get_edge() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(1, 3, true);
        assert!(g.edge(1, 3));
        assert!(g.edge(3, 1)); // symmetric
        assert!(!g.edge(0, 1));
        assert_eq!(g.num_edges(), 1);
    }

    #[test]
    fn self_loop_ignored() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(2, 2, true);
        assert!(!g.edge(2, 2));
        assert_eq!(g.num_edges(), 0);
    }

    #[test]
    fn complement_flips_all_edges() {
        let mut g = AdjacencyMatrix::new(4);
        g.set_edge(0, 1, true);
        g.set_edge(2, 3, true);
        let comp = g.complement();
        assert!(!comp.edge(0, 1));
        assert!(!comp.edge(2, 3));
        assert!(comp.edge(0, 2));
        assert!(comp.edge(0, 3));
        assert!(comp.edge(1, 2));
        assert!(comp.edge(1, 3));
        assert_eq!(comp.num_edges(), 4);
    }

    #[test]
    fn neighbors_correct() {
        let mut g = AdjacencyMatrix::new(4);
        g.set_edge(0, 1, true);
        g.set_edge(0, 3, true);
        let mut nbrs = g.neighbors(0);
        nbrs.sort();
        assert_eq!(nbrs, vec![1, 3]);
    }

    #[test]
    fn complete_graph_edge_count() {
        let n = 5u32;
        let mut g = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                g.set_edge(i, j, true);
            }
        }
        assert_eq!(g.num_edges(), 10);
    }

    #[test]
    fn permute_vertices_relabels() {
        let mut g = AdjacencyMatrix::new(4);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(2, 3, true);
        let perm = vec![3, 2, 1, 0];
        let h = g.permute_vertices(&perm);
        assert!(h.edge(0, 1));
        assert!(h.edge(1, 2));
        assert!(h.edge(2, 3));
        assert_eq!(h.num_edges(), 3);
    }

    #[test]
    fn neighbor_masks_correct() {
        let mut g = AdjacencyMatrix::new(4);
        g.set_edge(0, 1, true);
        g.set_edge(0, 3, true);
        g.set_edge(1, 2, true);
        let masks = g.neighbor_masks();
        assert_eq!(masks[0], (1 << 1) | (1 << 3)); // neighbors of 0: {1, 3}
        assert_eq!(masks[1], (1 << 0) | (1 << 2)); // neighbors of 1: {0, 2}
        assert_eq!(masks[2], 1 << 1); // neighbors of 2: {1}
        assert_eq!(masks[3], 1 << 0); // neighbors of 3: {0}
    }

    #[test]
    fn n_zero_and_one() {
        let g0 = AdjacencyMatrix::new(0);
        assert_eq!(g0.n(), 0);
        assert_eq!(g0.num_edges(), 0);

        let g1 = AdjacencyMatrix::new(1);
        assert_eq!(g1.n(), 1);
        assert_eq!(g1.num_edges(), 0);
    }
}
