use ramseynet_graph::AdjacencyMatrix;

/// Find a clique of size exactly `k` in the graph, returning the
/// lexicographically smallest such clique if one exists.
///
/// Uses backtracking search with vertices explored in ascending order,
/// so the first clique found of size k is guaranteed to be the lex-smallest.
pub fn find_clique_witness(adj: &AdjacencyMatrix, k: u32) -> Option<Vec<u32>> {
    if k == 0 {
        return Some(vec![]);
    }
    let n = adj.n();
    if k == 1 && n > 0 {
        return Some(vec![0]);
    }
    if k > n {
        return None;
    }

    let mut current = Vec::with_capacity(k as usize);
    if backtrack(adj, &mut current, 0, k) {
        Some(current)
    } else {
        None
    }
}

/// Backtracking search for a k-clique starting from vertex `start`.
/// Vertices are added in ascending order, guaranteeing lex-smallest result.
fn backtrack(adj: &AdjacencyMatrix, current: &mut Vec<u32>, start: u32, k: u32) -> bool {
    if current.len() as u32 == k {
        return true;
    }

    let remaining = k - current.len() as u32;
    let n = adj.n();

    // Pruning: not enough vertices left to complete the clique
    if n - start < remaining {
        return false;
    }

    for v in start..n {
        // Check that v is adjacent to all vertices already in the clique
        let connected_to_all = current.iter().all(|&u| adj.edge(u, v));
        if !connected_to_all {
            continue;
        }

        current.push(v);
        if backtrack(adj, current, v + 1, k) {
            return true;
        }
        current.pop();
    }

    false
}

/// Count the number of cliques of exactly size `k` in the graph.
///
/// Uses the same backtracking approach as `find_clique_witness` but
/// exhaustively enumerates all k-cliques instead of stopping at the first.
pub fn count_cliques(adj: &AdjacencyMatrix, k: u32) -> u64 {
    if k == 0 {
        return 1;
    }
    let n = adj.n();
    if k == 1 {
        return n as u64;
    }
    if k > n {
        return 0;
    }
    let mut current = Vec::with_capacity(k as usize);
    let mut count = 0u64;
    count_backtrack(adj, &mut current, 0, k, &mut count);
    count
}

fn count_backtrack(
    adj: &AdjacencyMatrix,
    current: &mut Vec<u32>,
    start: u32,
    k: u32,
    count: &mut u64,
) {
    if current.len() as u32 == k {
        *count += 1;
        return;
    }

    let remaining = k - current.len() as u32;
    let n = adj.n();

    if n - start < remaining {
        return;
    }

    for v in start..n {
        let connected_to_all = current.iter().all(|&u| adj.edge(u, v));
        if !connected_to_all {
            continue;
        }
        current.push(v);
        count_backtrack(adj, current, v + 1, k, count);
        current.pop();
    }
}

/// Clique number omega(G): size of the largest clique.
///
/// Iterates k=1,2,3,... calling `find_clique_witness` until it fails.
/// For Ramsey-relevant graph sizes (n≤20, k≤4) this terminates quickly.
pub fn max_clique_size(adj: &AdjacencyMatrix) -> u32 {
    let n = adj.n();
    if n == 0 {
        return 0;
    }
    let mut k = 1;
    while k <= n {
        if find_clique_witness(adj, k + 1).is_none() {
            return k;
        }
        k += 1;
    }
    n
}

/// Find the clique number and count all cliques of that maximum size.
/// Returns `(omega, count)`.
pub fn count_max_cliques(adj: &AdjacencyMatrix) -> (u32, u64) {
    let omega = max_clique_size(adj);
    let count = count_cliques(adj, omega);
    (omega, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clique_in_empty_graph() {
        let g = AdjacencyMatrix::new(5);
        assert!(find_clique_witness(&g, 2).is_none());
    }

    #[test]
    fn finds_edge_as_2_clique() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        let w = find_clique_witness(&g, 2).unwrap();
        assert_eq!(w, vec![0, 1]);
    }

    #[test]
    fn finds_triangle() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(0, 2, true);
        let w = find_clique_witness(&g, 3).unwrap();
        assert_eq!(w, vec![0, 1, 2]);
    }

    #[test]
    fn lex_smallest_triangle() {
        let mut g = AdjacencyMatrix::new(5);
        // Triangle on {0,1,2} and triangle on {2,3,4}
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(0, 2, true);
        g.set_edge(2, 3, true);
        g.set_edge(3, 4, true);
        g.set_edge(2, 4, true);
        let w = find_clique_witness(&g, 3).unwrap();
        assert_eq!(w, vec![0, 1, 2]);
    }

    #[test]
    fn no_3_clique_in_c5() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(2, 3, true);
        g.set_edge(3, 4, true);
        g.set_edge(4, 0, true);
        assert!(find_clique_witness(&g, 3).is_none());
    }

    #[test]
    fn trivial_k0_k1() {
        let g = AdjacencyMatrix::new(3);
        let w0 = find_clique_witness(&g, 0).unwrap();
        assert_eq!(w0, vec![]);
        let w1 = find_clique_witness(&g, 1).unwrap();
        assert_eq!(w1, vec![0]);
    }

    #[test]
    fn k_larger_than_n() {
        let g = AdjacencyMatrix::new(3);
        assert!(find_clique_witness(&g, 4).is_none());
    }

    #[test]
    fn complete_graph_has_full_clique() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        let w = find_clique_witness(&g, 5).unwrap();
        assert_eq!(w, vec![0, 1, 2, 3, 4]);
    }

    /// C5 has omega=2 (edges but no triangles).
    #[test]
    fn c5_max_clique_size() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        assert_eq!(max_clique_size(&g), 2);
    }

    /// K5 has omega=5.
    #[test]
    fn k5_max_clique_size() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        assert_eq!(max_clique_size(&g), 5);
    }

    /// C5 has 5 edges = 5 cliques of size 2.
    #[test]
    fn c5_count_max_cliques() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        let (omega, count) = count_max_cliques(&g);
        assert_eq!(omega, 2);
        assert_eq!(count, 5);
    }

    /// K5 has exactly 1 clique of size 5.
    #[test]
    fn k5_count_max_cliques() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        let (omega, count) = count_max_cliques(&g);
        assert_eq!(omega, 5);
        assert_eq!(count, 1);
    }

    /// Empty graph has omega=1 (isolated vertices are 1-cliques), count=n.
    #[test]
    fn empty_graph_max_clique() {
        let g = AdjacencyMatrix::new(5);
        let (omega, count) = count_max_cliques(&g);
        assert_eq!(omega, 1);
        assert_eq!(count, 5);
    }
}
