//! Incremental violation counting for Ramsey graph search.
//!
//! When flipping a single edge (u,v) in a graph, only cliques containing
//! both u and v can be affected. For R(k,ell) with n vertices, this means
//! examining C(n-2, k-2) potential sub-cliques instead of C(n, k). For
//! R(5,5) n=25: ~1,771 vs ~53,130 — a ~30x reduction.
//!
//! These functions are shared between the evo and tree2 strategies.

use ramseynet_graph::AdjacencyMatrix;

/// Count k-cliques that contain BOTH vertices u and v.
/// Returns 0 if edge (u,v) is not present (no clique can contain a non-edge).
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

/// Count cliques of size `target` using only vertices from `candidates`.
pub(crate) fn count_cliques_in_subset(
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

/// Count k-cliques through (u,v) assuming the (u,v) edge has a specific state.
/// This avoids mutating or cloning the adjacency matrix.
pub(crate) fn count_cliques_through_edge_assuming(
    adj: &AdjacencyMatrix,
    k: u32,
    u: u32,
    v: u32,
    edge_present: bool,
) -> u64 {
    if k < 2 {
        return 0;
    }
    if !edge_present {
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

/// Compute the change in violation score from flipping edge (u,v).
///
/// Takes both the graph and its pre-built complement to avoid allocations.
/// The caller is responsible for keeping `comp` in sync with `adj`.
///
/// Returns (delta_kc, delta_ei): the change in k-clique count and
/// ell-independent-set count. These can be negative (improvement).
pub(crate) fn violation_delta(
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
        // Removing edge from G: all k-cliques through (u,v) destroyed.
        // Adding edge to complement: count new ell-cliques.
        let ei_after = count_cliques_through_edge_assuming(comp, ell, u, v, true) as i64;
        (-kc_before, ei_after - ei_before)
    } else {
        // Adding edge to G: count new k-cliques.
        // Removing edge from complement: all ell-cliques through (u,v) destroyed.
        let kc_after = count_cliques_through_edge_assuming(adj, k, u, v, true) as i64;
        (kc_after - kc_before, -ei_before)
    }
}

/// Fast 64-bit fingerprint of an adjacency matrix for dedup.
///
/// XOR-folds the raw packed bits with multiplicative mixing.
/// Much cheaper than SHA-256 (~5ns vs ~200ns) and sufficient
/// for in-memory dedup (collision probability ~2^-64).
pub(crate) fn fast_fingerprint(adj: &AdjacencyMatrix) -> u64 {
    let bits = adj.packed_bits();
    let mut h: u64 = bits.len() as u64;
    // Process 8 bytes at a time
    let chunks = bits.chunks_exact(8);
    let remainder = chunks.remainder();
    for chunk in chunks {
        let word = u64::from_le_bytes(chunk.try_into().unwrap());
        h ^= word;
        h = h.wrapping_mul(0x517cc1b727220a95); // FxHash constant
    }
    // Handle remainder
    if !remainder.is_empty() {
        let mut buf = [0u8; 8];
        buf[..remainder.len()].copy_from_slice(remainder);
        let word = u64::from_le_bytes(buf);
        h ^= word;
        h = h.wrapping_mul(0x517cc1b727220a95);
    }
    h
}
