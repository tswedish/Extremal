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
}
