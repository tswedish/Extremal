//! Clique counting via bitwise neighbor bitmasks.
//!
//! Ported from the RamseyNet prototype's `incremental.rs` module. Provides
//! both full graph clique counting and per-edge clique counting for
//! incremental delta computation.
//!
//! All operations require n <= 64 (neighbor sets fit in a single u64).

use extremal_graph::AdjacencyMatrix;

// ══════════════════════════════════════════════════════════════
// NeighborSet — bitwise neighbor masks for n <= 64
// ══════════════════════════════════════════════════════════════

/// Precomputed neighbor bitmasks for fast set operations.
///
/// `masks[v]` has bit `w` set iff edge(v, w) exists.
/// Supports n <= 64 (R(5,5) n=25 fits in a single u64).
#[derive(Clone, Debug)]
pub struct NeighborSet {
    pub masks: Vec<u64>,
}

impl NeighborSet {
    /// Build from an AdjacencyMatrix (O(n^2)).
    pub fn from_adj(adj: &AdjacencyMatrix) -> Self {
        Self {
            masks: adj.neighbor_masks(),
        }
    }

    /// Toggle the (u,v) edge in both directions. Two XOR ops, zero allocation.
    #[inline]
    pub fn flip_edge(&mut self, u: u32, v: u32) {
        self.masks[u as usize] ^= 1u64 << v;
        self.masks[v as usize] ^= 1u64 << u;
    }

    /// Check if edge (u,v) exists.
    #[inline]
    pub fn has_edge(&self, u: u32, v: u32) -> bool {
        self.masks[u as usize] & (1u64 << v) != 0
    }
}

// ══════════════════════════════════════════════════════════════
// Full graph clique counting
// ══════════════════════════════════════════════════════════════

/// Count all k-cliques in the graph defined by `nbrs` on `n` vertices.
///
/// For counting monochromatic cliques in a 2-coloring, call this once on
/// the graph's neighbor set and once on the complement's neighbor set.
pub fn count_cliques(nbrs: &NeighborSet, k: u32, n: u32) -> u64 {
    if k == 0 {
        return 1;
    }
    if k == 1 {
        return n as u64;
    }
    if k == 2 {
        // Count edges
        let mut count = 0u64;
        for v in 0..n {
            let higher = nbrs.masks[v as usize] & !((1u64 << (v + 1)) - 1);
            count += higher.count_ones() as u64;
        }
        return count;
    }

    // General: enumerate k-cliques starting from each vertex
    let mut total = 0u64;
    for v in 0..n {
        // Candidates: neighbors of v with index > v (avoid double counting)
        let candidates = nbrs.masks[v as usize] & !((1u64 << (v + 1)) - 1);
        total += count_cliques_in_mask(nbrs, candidates, k - 1);
    }
    total
}

/// Count k-cliques containing both u and v, using bitmask operations.
///
/// Returns 0 if edge (u,v) is not present.
#[inline]
pub fn count_cliques_through_edge(nbrs: &NeighborSet, k: u32, u: u32, v: u32) -> u64 {
    if k < 2 || !nbrs.has_edge(u, v) {
        return 0;
    }
    if k == 2 {
        return 1;
    }
    let common = nbrs.masks[u as usize] & nbrs.masks[v as usize] & !(1u64 << u) & !(1u64 << v);
    if common.count_ones() < k - 2 {
        return 0;
    }
    count_cliques_in_mask(nbrs, common, k - 2)
}

/// Count k-cliques containing both u and v, assuming the (u,v) edge exists
/// even if it doesn't in `nbrs`. Used for "what-if" delta computation.
#[inline]
pub fn count_cliques_through_edge_assuming(
    nbrs: &NeighborSet,
    k: u32,
    u: u32,
    v: u32,
    edge_present: bool,
) -> u64 {
    if k < 2 || !edge_present {
        return 0;
    }
    if k == 2 {
        return 1;
    }
    let common = nbrs.masks[u as usize] & nbrs.masks[v as usize] & !(1u64 << u) & !(1u64 << v);
    if common.count_ones() < k - 2 {
        return 0;
    }
    count_cliques_in_mask(nbrs, common, k - 2)
}

// ══════════════════════════════════════════════════════════════
// Violation delta (for search strategies)
// ══════════════════════════════════════════════════════════════

/// Compute the change in violation score from flipping edge (u,v).
///
/// Returns (delta_kc, delta_ei) where:
/// - delta_kc = change in k-clique count in the graph
/// - delta_ei = change in ell-clique count in the complement
///
/// Uses bitwise neighbor masks — no heap allocation.
pub fn violation_delta(
    adj_nbrs: &NeighborSet,
    comp_nbrs: &NeighborSet,
    k: u32,
    ell: u32,
    u: u32,
    v: u32,
) -> (i64, i64) {
    let edge_present = adj_nbrs.has_edge(u, v);

    let kc_before = count_cliques_through_edge(adj_nbrs, k, u, v) as i64;
    let ei_before = count_cliques_through_edge(comp_nbrs, ell, u, v) as i64;

    if edge_present {
        // Removing edge from G: all k-cliques through (u,v) destroyed
        // Adding edge to complement: count new ell-cliques
        let ei_after = count_cliques_through_edge_assuming(comp_nbrs, ell, u, v, true) as i64;
        (-kc_before, ei_after - ei_before)
    } else {
        // Adding edge to G: count new k-cliques
        // Removing edge from complement: all ell-cliques destroyed
        let kc_after = count_cliques_through_edge_assuming(adj_nbrs, k, u, v, true) as i64;
        (kc_after - kc_before, -ei_before)
    }
}

// ══════════════════════════════════════════════════════════════
// Fast fingerprint (for dedup)
// ══════════════════════════════════════════════════════════════

/// 64-bit XOR-fold fingerprint for fast dedup during search.
///
/// NOT cryptographic. Two distinct graphs can (rarely) collide.
/// Used for beam dedup where false positives just waste one
/// candidate slot — acceptable trade-off for 40x speedup over blake3.
pub fn fast_fingerprint(masks: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    for &m in masks {
        h ^= m;
        h = h.wrapping_mul(0x100000001b3); // FNV prime
    }
    h
}

// ══════════════════════════════════════════════════════════════
// Guilty edges (focused edge flipping)
// ══════════════════════════════════════════════════════════════

/// Collect all edges that participate in at least one monochromatic k-clique
/// (in the graph) or ell-clique (in the complement).
///
/// Returns a deduplicated list of (u, v) pairs with u < v.
pub fn guilty_edges(
    adj_nbrs: &NeighborSet,
    comp_nbrs: &NeighborSet,
    k: u32,
    ell: u32,
    n: u32,
) -> Vec<(u32, u32)> {
    let mut guilty = vec![0u64; n as usize];
    enumerate_cliques_and_mark(adj_nbrs, k, n, &mut guilty);
    enumerate_cliques_and_mark(comp_nbrs, ell, n, &mut guilty);

    let mut edges = Vec::new();
    for u in 0..n {
        let mask = guilty[u as usize] & !((1u64 << (u + 1)) - 1);
        let mut m = mask;
        while m != 0 {
            let v = m.trailing_zeros();
            m &= m - 1;
            edges.push((u, v));
        }
    }
    edges
}

// ══════════════════════════════════════════════════════════════
// Internal helpers
// ══════════════════════════════════════════════════════════════

/// Count cliques of size `target` among the vertices in `candidates` bitmask.
///
/// Specialized fast paths for target=1,2,3 (the R(5,5) hot path).
fn count_cliques_in_mask(nbrs: &NeighborSet, candidates: u64, target: u32) -> u64 {
    match target {
        0 => 1,
        1 => candidates.count_ones() as u64,
        2 => {
            let mut count = 0u64;
            let mut mask = candidates;
            while mask != 0 {
                let v = mask.trailing_zeros();
                mask &= mask - 1;
                let higher = candidates & nbrs.masks[v as usize] & !((1u64 << (v + 1)) - 1);
                count += higher.count_ones() as u64;
            }
            count
        }
        3 => {
            // Count triangles — the R(5,5) hot path
            let mut count = 0u64;
            let mut mask_a = candidates;
            while mask_a != 0 {
                let a = mask_a.trailing_zeros();
                mask_a &= mask_a - 1;
                let nbrs_a_in_cand = nbrs.masks[a as usize] & candidates & !((1u64 << (a + 1)) - 1);
                let mut mask_b = nbrs_a_in_cand;
                while mask_b != 0 {
                    let b = mask_b.trailing_zeros();
                    mask_b &= mask_b - 1;
                    let nbrs_ab =
                        nbrs_a_in_cand & nbrs.masks[b as usize] & !((1u64 << (b + 1)) - 1);
                    count += nbrs_ab.count_ones() as u64;
                }
            }
            count
        }
        _ => {
            let mut count = 0u64;
            let mut mask = candidates;
            while mask != 0 {
                let v = mask.trailing_zeros();
                mask &= mask - 1;
                let sub = mask & nbrs.masks[v as usize];
                count += count_cliques_in_mask(nbrs, sub, target - 1);
            }
            count
        }
    }
}

/// Enumerate all k-cliques and mark all participating edges in `guilty`.
fn enumerate_cliques_and_mark(nbrs: &NeighborSet, k: u32, n: u32, guilty: &mut [u64]) {
    if k < 2 {
        return;
    }
    if k == 2 {
        for u in 0..n {
            guilty[u as usize] |= nbrs.masks[u as usize];
        }
        return;
    }
    let mut clique = Vec::with_capacity(k as usize);
    for v in 0..n {
        clique.clear();
        clique.push(v);
        let candidates = nbrs.masks[v as usize] & !((1u64 << (v + 1)) - 1);
        enumerate_and_mark_recurse(nbrs, k, &mut clique, candidates, guilty);
    }
}

/// Recursive helper: extend `clique` to size `k`, mark edges when complete.
fn enumerate_and_mark_recurse(
    nbrs: &NeighborSet,
    k: u32,
    clique: &mut Vec<u32>,
    candidates: u64,
    guilty: &mut [u64],
) {
    if clique.len() as u32 == k {
        for i in 0..clique.len() {
            for j in (i + 1)..clique.len() {
                let u = clique[i];
                let v = clique[j];
                guilty[u as usize] |= 1u64 << v;
                guilty[v as usize] |= 1u64 << u;
            }
        }
        return;
    }
    let remaining = k - clique.len() as u32;
    if candidates.count_ones() < remaining {
        return;
    }
    let mut mask = candidates;
    while mask != 0 {
        let w = mask.trailing_zeros();
        mask &= mask - 1;
        clique.push(w);
        let next = mask & nbrs.masks[w as usize];
        enumerate_and_mark_recurse(nbrs, k, clique, next, guilty);
        clique.pop();
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
    fn k5_has_one_5clique() {
        let g = make_k5();
        let nbrs = NeighborSet::from_adj(&g);
        assert_eq!(count_cliques(&nbrs, 5, 5), 1);
        assert_eq!(count_cliques(&nbrs, 4, 5), 5);
        assert_eq!(count_cliques(&nbrs, 3, 5), 10);
        assert_eq!(count_cliques(&nbrs, 2, 5), 10);
        assert_eq!(count_cliques(&nbrs, 1, 5), 5);
    }

    #[test]
    fn c5_has_no_triangles() {
        let g = make_c5();
        let nbrs = NeighborSet::from_adj(&g);
        assert_eq!(count_cliques(&nbrs, 3, 5), 0);
        assert_eq!(count_cliques(&nbrs, 2, 5), 5);
    }

    #[test]
    fn empty_graph_no_cliques() {
        let g = AdjacencyMatrix::new(10);
        let nbrs = NeighborSet::from_adj(&g);
        assert_eq!(count_cliques(&nbrs, 3, 10), 0);
        assert_eq!(count_cliques(&nbrs, 2, 10), 0);
    }

    #[test]
    fn cliques_through_edge_k5() {
        let g = make_k5();
        let nbrs = NeighborSet::from_adj(&g);
        // Every edge in K5 is in exactly 1 5-clique
        assert_eq!(count_cliques_through_edge(&nbrs, 5, 0, 1), 1);
        // Every edge is in C(3,2)=3 4-cliques
        assert_eq!(count_cliques_through_edge(&nbrs, 4, 0, 1), 3);
        // Every edge is in C(3,1)=3 triangles
        assert_eq!(count_cliques_through_edge(&nbrs, 3, 0, 1), 3);
    }

    #[test]
    fn violation_delta_flipping_edge_in_k5() {
        let g = make_k5();
        let comp = g.complement();
        let adj_nbrs = NeighborSet::from_adj(&g);
        let comp_nbrs = NeighborSet::from_adj(&comp);

        // Removing an edge from K5: destroys the 5-clique (and others)
        let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, 5, 5, 0, 1);
        assert!(dk < 0, "removing edge should reduce k-cliques");
        assert_eq!(dk, -1, "K5 has exactly one 5-clique through any edge");
        // Adding edge to complement: complement of K5 is empty, so adding
        // one edge can't create any 5-independent-set
        assert_eq!(de, 0);
    }

    #[test]
    fn fingerprint_deterministic() {
        let g = make_c5();
        let masks = g.neighbor_masks();
        let fp1 = fast_fingerprint(&masks);
        let fp2 = fast_fingerprint(&masks);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_different_graphs() {
        let g1 = make_k5();
        let g2 = make_c5();
        let fp1 = fast_fingerprint(&g1.neighbor_masks());
        let fp2 = fast_fingerprint(&g2.neighbor_masks());
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn guilty_edges_of_k5() {
        let g = make_k5();
        let comp = g.complement();
        let adj_nbrs = NeighborSet::from_adj(&g);
        let comp_nbrs = NeighborSet::from_adj(&comp);
        let guilty = guilty_edges(&adj_nbrs, &comp_nbrs, 5, 5, 5);
        // K5 has one 5-clique, all 10 edges are guilty
        assert_eq!(guilty.len(), 10);
    }

    #[test]
    fn guilty_edges_of_empty_graph() {
        let g = AdjacencyMatrix::new(5);
        let comp = g.complement(); // K5
        let adj_nbrs = NeighborSet::from_adj(&g);
        let comp_nbrs = NeighborSet::from_adj(&comp);
        let guilty = guilty_edges(&adj_nbrs, &comp_nbrs, 5, 5, 5);
        // Complement is K5 with one 5-clique, all 10 edges are guilty
        assert_eq!(guilty.len(), 10);
    }
}
