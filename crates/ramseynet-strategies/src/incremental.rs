//! Incremental violation counting for Ramsey graph search.
//!
//! Two implementations:
//!
//! - **Scalar** (original): Per-vertex `edge()` calls, recursive backtracking.
//!   Kept as a test oracle.
//! - **Bitwise** (new): Neighbor bitmasks, AND/popcount operations. For R(5,5)
//!   n=25, the entire neighbor set fits in a single `u64`. Common-neighbor
//!   intersection is one AND instruction. Clique enumeration is nested bit
//!   iteration with zero heap allocations.
//!
//! The bitwise path is ~5-15x faster for the hot loop.

use ramseynet_graph::AdjacencyMatrix;

// ══════════════════════════════════════════════════════════════
// NeighborSet — bitwise neighbor masks for n ≤ 64
// ══════════════════════════════════════════════════════════════

/// Precomputed neighbor bitmasks for fast set operations.
///
/// `masks[v]` has bit `w` set iff edge(v, w) exists.
/// Supports n ≤ 64 (R(5,5) n=25 and even n=48 fit in a single u64).
#[derive(Clone, Debug)]
pub(crate) struct NeighborSet {
    pub masks: Vec<u64>,
}

impl NeighborSet {
    /// Build from an AdjacencyMatrix (O(n²)).
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
// Bitwise clique counting
// ══════════════════════════════════════════════════════════════

/// Count k-cliques containing both u and v, using bitmask operations.
///
/// Returns 0 if edge (u,v) is not present.
/// For k=5 n=25: common neighbors ≈ 12, so the triple-nested bit
/// iteration examines ~C(12,3)=220 triples, each costing one AND +
/// one popcount. Total: ~600 bit ops vs ~3000 scalar ops.
#[inline]
pub(crate) fn count_cliques_through_edge_bw(nbrs: &NeighborSet, k: u32, u: u32, v: u32) -> u64 {
    if k < 2 {
        return 0;
    }
    if !nbrs.has_edge(u, v) {
        return 0;
    }
    if k == 2 {
        return 1;
    }
    // Common neighbors of u and v, excluding u and v themselves
    let common = nbrs.masks[u as usize] & nbrs.masks[v as usize] & !(1u64 << u) & !(1u64 << v);
    if common.count_ones() < k - 2 {
        return 0;
    }
    count_cliques_in_mask(nbrs, common, k - 2)
}

/// Count k-cliques containing both u and v, assuming the (u,v) edge exists
/// even if it doesn't in `nbrs`. Used for "what-if" delta computation.
#[inline]
pub(crate) fn count_cliques_through_edge_assuming_bw(
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
    // Common neighbors: use actual masks (other edges unchanged)
    let common = nbrs.masks[u as usize] & nbrs.masks[v as usize] & !(1u64 << u) & !(1u64 << v);
    if common.count_ones() < k - 2 {
        return 0;
    }
    count_cliques_in_mask(nbrs, common, k - 2)
}

/// Count cliques of size `target` among the vertices in `candidates` bitmask.
///
/// Specialized fast paths for target=1,2,3 (the R(5,5) hot path).
/// General recursive fallback for larger targets.
fn count_cliques_in_mask(nbrs: &NeighborSet, candidates: u64, target: u32) -> u64 {
    match target {
        0 => 1,
        1 => candidates.count_ones() as u64,
        2 => {
            // Count edges among candidates
            let mut count = 0u64;
            let mut mask = candidates;
            while mask != 0 {
                let v = mask.trailing_zeros();
                mask &= mask - 1; // clear lowest bit
                                  // Neighbors of v that are in candidates AND have index > v
                let higher = candidates & nbrs.masks[v as usize] & !((1u64 << (v + 1)) - 1);
                count += higher.count_ones() as u64;
            }
            count
        }
        3 => {
            // Count triangles among candidates — the R(5,5) hot path
            let mut count = 0u64;
            let mut mask_a = candidates;
            while mask_a != 0 {
                let a = mask_a.trailing_zeros();
                mask_a &= mask_a - 1;
                // b must be neighbor of a, in candidates, index > a
                let nbrs_a_in_cand = nbrs.masks[a as usize] & candidates & !((1u64 << (a + 1)) - 1);
                let mut mask_b = nbrs_a_in_cand;
                while mask_b != 0 {
                    let b = mask_b.trailing_zeros();
                    mask_b &= mask_b - 1;
                    // c must be neighbor of both a and b, in candidates, index > b
                    let nbrs_ab =
                        nbrs_a_in_cand & nbrs.masks[b as usize] & !((1u64 << (b + 1)) - 1);
                    count += nbrs_ab.count_ones() as u64;
                }
            }
            count
        }
        _ => {
            // General case: recursive bitmask enumeration
            let mut count = 0u64;
            let mut mask = candidates;
            while mask != 0 {
                let v = mask.trailing_zeros();
                mask &= mask - 1;
                // Remaining candidates: in mask (higher index) AND neighbors of v
                let sub = mask & nbrs.masks[v as usize];
                count += count_cliques_in_mask(nbrs, sub, target - 1);
            }
            count
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Bitwise violation delta
// ══════════════════════════════════════════════════════════════

/// Compute the change in violation score from flipping edge (u,v).
///
/// Uses bitwise neighbor masks — no heap allocation, no AdjacencyMatrix
/// access in the hot path. The caller maintains `adj_nbrs` and `comp_nbrs`
/// in sync with the actual graph and complement.
///
/// Returns (delta_kc, delta_ei).
pub(crate) fn violation_delta_bitwise(
    adj_nbrs: &NeighborSet,
    comp_nbrs: &NeighborSet,
    k: u32,
    ell: u32,
    u: u32,
    v: u32,
) -> (i64, i64) {
    let edge_present = adj_nbrs.has_edge(u, v);

    let kc_before = count_cliques_through_edge_bw(adj_nbrs, k, u, v) as i64;
    let ei_before = count_cliques_through_edge_bw(comp_nbrs, ell, u, v) as i64;

    if edge_present {
        // Removing edge from G → all k-cliques through (u,v) destroyed
        // Adding edge to complement → count new ell-cliques
        let ei_after = count_cliques_through_edge_assuming_bw(comp_nbrs, ell, u, v, true) as i64;
        (-kc_before, ei_after - ei_before)
    } else {
        // Adding edge to G → count new k-cliques
        // Removing edge from complement → all ell-cliques destroyed
        let kc_after = count_cliques_through_edge_assuming_bw(adj_nbrs, k, u, v, true) as i64;
        (kc_after - kc_before, -ei_before)
    }
}

// ══════════════════════════════════════════════════════════════
// Scalar implementations (kept as test oracles)
// ══════════════════════════════════════════════════════════════

/// Scalar: count k-cliques containing both u and v.
#[cfg(test)]
pub(crate) fn count_cliques_through_edge(adj: &AdjacencyMatrix, k: u32, u: u32, v: u32) -> u64 {
    if k < 2 {
        return 0;
    }
    if !adj.edge(u, v) {
        return 0;
    }
    if k == 2 {
        return 1;
    }
    let n = adj.n();
    let common: Vec<u32> = (0..n)
        .filter(|&w| w != u && w != v && adj.edge(u, w) && adj.edge(v, w))
        .collect();
    if (common.len() as u32) < k - 2 {
        return 0;
    }
    let mut count = 0u64;
    let mut current = Vec::with_capacity((k - 2) as usize);
    count_cliques_in_subset(adj, &common, &mut current, 0, k - 2, &mut count);
    count
}

#[cfg(test)]
fn count_cliques_in_subset(
    adj: &AdjacencyMatrix,
    candidates: &[u32],
    current: &mut Vec<u32>,
    start: usize,
    target: u32,
    count: &mut u64,
) {
    if current.len() as u32 == target {
        *count += 1;
        return;
    }
    let remaining = target - current.len() as u32;
    if candidates.len() - start < remaining as usize {
        return;
    }
    for i in start..candidates.len() {
        let v = candidates[i];
        if current.iter().all(|&u| adj.edge(u, v)) {
            current.push(v);
            count_cliques_in_subset(adj, candidates, current, i + 1, target, count);
            current.pop();
        }
    }
}

/// Scalar: violation delta (test oracle).
#[cfg(test)]
pub(crate) fn violation_delta_scalar(
    adj: &AdjacencyMatrix,
    comp: &AdjacencyMatrix,
    k: u32,
    ell: u32,
    u: u32,
    v: u32,
) -> (i64, i64) {
    let edge_present = adj.edge(u, v);
    let kc_before = count_cliques_through_edge(adj, k, u, v) as i64;
    let ei_before = count_cliques_through_edge(comp, ell, u, v) as i64;
    if edge_present {
        let ei_after = count_cliques_through_edge_assuming_scalar(comp, ell, u, v, true) as i64;
        (-kc_before, ei_after - ei_before)
    } else {
        let kc_after = count_cliques_through_edge_assuming_scalar(adj, k, u, v, true) as i64;
        (kc_after - kc_before, -ei_before)
    }
}

#[cfg(test)]
fn count_cliques_through_edge_assuming_scalar(
    adj: &AdjacencyMatrix,
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
    let n = adj.n();
    let common: Vec<u32> = (0..n)
        .filter(|&w| w != u && w != v && adj.edge(u, w) && adj.edge(v, w))
        .collect();
    if (common.len() as u32) < k - 2 {
        return 0;
    }
    let mut count = 0u64;
    let mut current = Vec::with_capacity((k - 2) as usize);
    count_cliques_in_subset(adj, &common, &mut current, 0, k - 2, &mut count);
    count
}

// ══════════════════════════════════════════════════════════════
// Fast fingerprint (unchanged)
// ══════════════════════════════════════════════════════════════

/// Fast 64-bit fingerprint of an adjacency matrix for dedup.
pub(crate) fn fast_fingerprint(adj: &AdjacencyMatrix) -> u64 {
    let bits = adj.packed_bits();
    let mut h: u64 = bits.len() as u64;
    let chunks = bits.chunks_exact(8);
    let remainder = chunks.remainder();
    for chunk in chunks {
        let word = u64::from_le_bytes(chunk.try_into().unwrap());
        h ^= word;
        h = h.wrapping_mul(0x517cc1b727220a95);
    }
    if !remainder.is_empty() {
        let mut buf = [0u8; 8];
        buf[..remainder.len()].copy_from_slice(remainder);
        let word = u64::from_le_bytes(buf);
        h ^= word;
        h = h.wrapping_mul(0x517cc1b727220a95);
    }
    h
}

// ══════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    fn random_graph(n: u32, seed: u64) -> AdjacencyMatrix {
        let mut rng = SmallRng::seed_from_u64(seed);
        let mut g = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                if rng.gen_bool(0.5) {
                    g.set_edge(i, j, true);
                }
            }
        }
        g
    }

    fn paley_graph(n: u32) -> AdjacencyMatrix {
        let p = {
            let mut p = n.max(5);
            loop {
                if p % 4 == 1 && is_prime(p) {
                    break p;
                }
                p += 1;
            }
        };
        let mut qr = vec![false; p as usize];
        for x in 1..p {
            qr[((x as u64 * x as u64) % p as u64) as usize] = true;
        }
        let mut g = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                let diff = ((i as i64 - j as i64).rem_euclid(p as i64)) as u32;
                if qr[diff as usize] {
                    g.set_edge(i, j, true);
                }
            }
        }
        g
    }

    fn is_prime(n: u32) -> bool {
        if n < 2 {
            return false;
        }
        if n < 4 {
            return true;
        }
        if n.is_multiple_of(2) || n.is_multiple_of(3) {
            return false;
        }
        let mut i = 5u32;
        while i * i <= n {
            if n.is_multiple_of(i) || n.is_multiple_of(i + 2) {
                return false;
            }
            i += 6;
        }
        true
    }

    /// Verify bitwise clique count matches scalar for many random edge queries.
    #[test]
    fn bitwise_clique_count_matches_scalar_k3_n10() {
        let g = random_graph(10, 111);
        let nbrs = NeighborSet::from_adj(&g);
        for u in 0..10u32 {
            for v in (u + 1)..10 {
                let scalar = count_cliques_through_edge(&g, 3, u, v);
                let bitwise = count_cliques_through_edge_bw(&nbrs, 3, u, v);
                assert_eq!(scalar, bitwise, "k=3 mismatch at ({u},{v})");
            }
        }
    }

    #[test]
    fn bitwise_clique_count_matches_scalar_k5_n25() {
        let g = paley_graph(25);
        let nbrs = NeighborSet::from_adj(&g);
        let mut rng = SmallRng::seed_from_u64(222);
        // Test 50 random edge pairs
        for _ in 0..50 {
            let u = rng.gen_range(0..25u32);
            let v = rng.gen_range(0..25u32);
            if u == v {
                continue;
            }
            let (u, v) = if u < v { (u, v) } else { (v, u) };
            let scalar = count_cliques_through_edge(&g, 5, u, v);
            let bitwise = count_cliques_through_edge_bw(&nbrs, 5, u, v);
            assert_eq!(scalar, bitwise, "k=5 mismatch at ({u},{v})");
        }
    }

    /// Verify bitwise violation_delta matches scalar for random flips.
    #[test]
    fn bitwise_delta_matches_scalar_k3_n10() {
        let mut rng = SmallRng::seed_from_u64(333);
        let mut g = random_graph(10, 333);
        let mut comp = g.complement();
        let mut adj_nbrs = NeighborSet::from_adj(&g);
        let mut comp_nbrs = NeighborSet::from_adj(&comp);

        for _ in 0..100 {
            let u = rng.gen_range(0..10u32);
            let v = rng.gen_range(0..10u32);
            if u == v {
                continue;
            }
            let (u, v) = if u < v { (u, v) } else { (v, u) };

            let (skc, sei) = violation_delta_scalar(&g, &comp, 3, 3, u, v);
            let (bkc, bei) = violation_delta_bitwise(&adj_nbrs, &comp_nbrs, 3, 3, u, v);
            assert_eq!((skc, sei), (bkc, bei), "delta mismatch at ({u},{v})");

            // Apply flip to keep everything in sync
            let cur = g.edge(u, v);
            g.set_edge(u, v, !cur);
            comp.set_edge(u, v, cur);
            adj_nbrs.flip_edge(u, v);
            comp_nbrs.flip_edge(u, v);
        }
    }

    #[test]
    fn bitwise_delta_matches_scalar_k5_n25() {
        let mut rng = SmallRng::seed_from_u64(555);
        let mut g = paley_graph(25);
        let mut comp = g.complement();
        let mut adj_nbrs = NeighborSet::from_adj(&g);
        let mut comp_nbrs = NeighborSet::from_adj(&comp);

        for _ in 0..30 {
            let u = rng.gen_range(0..25u32);
            let v = rng.gen_range(0..25u32);
            if u == v {
                continue;
            }
            let (u, v) = if u < v { (u, v) } else { (v, u) };

            let (skc, sei) = violation_delta_scalar(&g, &comp, 5, 5, u, v);
            let (bkc, bei) = violation_delta_bitwise(&adj_nbrs, &comp_nbrs, 5, 5, u, v);
            assert_eq!((skc, sei), (bkc, bei), "k=5 delta mismatch at ({u},{v})");

            let cur = g.edge(u, v);
            g.set_edge(u, v, !cur);
            comp.set_edge(u, v, cur);
            adj_nbrs.flip_edge(u, v);
            comp_nbrs.flip_edge(u, v);
        }
    }

    /// Verify NeighborSet stays in sync after many flips.
    #[test]
    fn neighbor_set_flip_stays_in_sync() {
        let mut rng = SmallRng::seed_from_u64(777);
        let mut g = random_graph(15, 777);
        let mut nbrs = NeighborSet::from_adj(&g);

        for _ in 0..200 {
            let u = rng.gen_range(0..15u32);
            let v = rng.gen_range(0..15u32);
            if u == v {
                continue;
            }
            let (u, v) = if u < v { (u, v) } else { (v, u) };
            let cur = g.edge(u, v);
            g.set_edge(u, v, !cur);
            nbrs.flip_edge(u, v);
        }

        // Verify masks match rebuilt
        let fresh = NeighborSet::from_adj(&g);
        for v in 0..15u32 {
            assert_eq!(
                nbrs.masks[v as usize], fresh.masks[v as usize],
                "mask mismatch at vertex {v}"
            );
        }
    }
}
