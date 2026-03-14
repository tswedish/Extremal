//! Automorphism group computation via nauty's `densenauty`.

use std::os::raw::c_int;

use nauty_Traces_sys::*;
use ramseynet_graph::AdjacencyMatrix;

/// Run nauty's `densenauty` on a graph and return the canonical labeling + group order.
///
/// Returns `(lab, aut_order)` where:
/// - `lab[i]` is the original vertex at position `i` in the canonical labeling
/// - `aut_order` is `|Aut(G)|` as f64 (grpsize1 * 10^grpsize2)
fn run_nauty(adj: &AdjacencyMatrix) -> (Vec<i32>, f64) {
    let n = adj.n() as usize;
    if n == 0 {
        return (vec![], 1.0);
    }

    let m = SETWORDSNEEDED(n);

    unsafe {
        nauty_check(
            WORDSIZE as c_int,
            m as c_int,
            n as c_int,
            NAUTYVERSIONID as c_int,
        );
    }

    let mut options = optionstruct {
        writeautoms: FALSE,
        getcanon: TRUE,
        ..optionblk::default()
    };
    let mut stats = statsblk::default();

    let mut lab = vec![0i32; n];
    let mut ptn = vec![0i32; n];
    let mut orbits = vec![0i32; n];

    let mut g = empty_graph(m, n);
    let mut canong = empty_graph(m, n);

    // Convert AdjacencyMatrix to nauty dense format
    for i in 0..n as u32 {
        for j in (i + 1)..adj.n() {
            if adj.edge(i, j) {
                ADDONEEDGE(&mut g, i as usize, j as usize, m);
            }
        }
    }

    unsafe {
        densenauty(
            g.as_mut_ptr(),
            lab.as_mut_ptr(),
            ptn.as_mut_ptr(),
            orbits.as_mut_ptr(),
            &mut options,
            &mut stats,
            m as c_int,
            n as c_int,
            canong.as_mut_ptr(),
        );
    }

    let aut_order = stats.grpsize1 * 10f64.powi(stats.grpsize2);
    (lab, aut_order)
}

/// Compute |Aut(G)| using nauty's densenauty.
///
/// Returns the automorphism group order as f64 (grpsize1 * 10^grpsize2).
/// For small graphs (n≤20) this is exact when the group order fits in f64.
pub fn automorphism_group_order(adj: &AdjacencyMatrix) -> f64 {
    let (_lab, aut_order) = run_nauty(adj);
    aut_order
}

/// Compute the canonical form of a graph under nauty's canonical labeling.
///
/// Returns `(canonical_matrix, aut_order)` where:
/// - `canonical_matrix` is the graph relabeled so that isomorphic graphs
///   produce identical adjacency matrices
/// - `aut_order` is `|Aut(G)|` (computed in the same nauty call)
///
/// Two graphs G and H are isomorphic iff `canonical_form(G).0 == canonical_form(H).0`.
pub fn canonical_form(adj: &AdjacencyMatrix) -> (AdjacencyMatrix, f64) {
    let n = adj.n() as usize;
    if n == 0 {
        return (AdjacencyMatrix::new(0), 1.0);
    }

    let (lab, aut_order) = run_nauty(adj);

    // lab[i] = original vertex at canonical position i.
    // Build inverse: inv_lab[original_vertex] = canonical_position.
    let mut inv_lab = vec![0u32; n];
    for (canon_pos, &orig_vertex) in lab.iter().enumerate() {
        inv_lab[orig_vertex as usize] = canon_pos as u32;
    }

    // Relabel: the canonical graph has edge (inv_lab[i], inv_lab[j]) iff
    // the original has edge (i, j).
    let canonical = adj.permute_vertices(&inv_lab);
    (canonical, aut_order)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C5 (5-cycle) has Aut = D5, |Aut| = 10.
    #[test]
    fn c5_automorphism_group() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        let order = automorphism_group_order(&g);
        assert_eq!(order, 10.0);
    }

    /// K5 has Aut = S5, |Aut| = 120.
    #[test]
    fn k5_automorphism_group() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        let order = automorphism_group_order(&g);
        assert_eq!(order, 120.0);
    }

    /// Paley(17) has |Aut| = 136 = 17 * 8 (affine maps x -> ax+b, a in QR(17)).
    #[test]
    fn paley17_automorphism_group() {
        let mut g = AdjacencyMatrix::new(17);
        // Quadratic residues mod 17: {1, 2, 4, 8, 9, 13, 15, 16}
        let qr: std::collections::HashSet<u32> = [1, 2, 4, 8, 9, 13, 15, 16].into_iter().collect();
        for i in 0..17u32 {
            for j in (i + 1)..17 {
                let diff = j.abs_diff(i);
                let d = diff.min(17 - diff);
                if qr.contains(&d) {
                    g.set_edge(i, j, true);
                }
            }
        }
        let order = automorphism_group_order(&g);
        assert_eq!(order, 136.0);
    }

    /// Empty graph on n vertices has |Aut| = n! (symmetric group).
    #[test]
    fn empty_graph_automorphism() {
        let g = AdjacencyMatrix::new(5);
        let order = automorphism_group_order(&g);
        assert_eq!(order, 120.0); // 5! = 120
    }

    /// Single vertex has trivial automorphism group.
    #[test]
    fn single_vertex() {
        let g = AdjacencyMatrix::new(1);
        let order = automorphism_group_order(&g);
        assert_eq!(order, 1.0);
    }

    /// Empty graph (0 vertices).
    #[test]
    fn zero_vertices() {
        let g = AdjacencyMatrix::new(0);
        let order = automorphism_group_order(&g);
        assert_eq!(order, 1.0);
    }

    /// Two isomorphic C5 graphs (different vertex orderings) produce the same canonical form.
    #[test]
    fn canonical_form_isomorphic_c5() {
        // C5 with standard labeling: 0-1-2-3-4-0
        let mut g1 = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g1.set_edge(i, (i + 1) % 5, true);
        }

        // C5 with different labeling: 0-2-4-1-3-0
        let mut g2 = AdjacencyMatrix::new(5);
        g2.set_edge(0, 2, true);
        g2.set_edge(2, 4, true);
        g2.set_edge(4, 1, true);
        g2.set_edge(1, 3, true);
        g2.set_edge(3, 0, true);

        let (canon1, aut1) = canonical_form(&g1);
        let (canon2, aut2) = canonical_form(&g2);

        assert_eq!(
            canon1, canon2,
            "isomorphic C5s must have same canonical form"
        );
        assert_eq!(aut1, aut2);
        assert_eq!(aut1, 10.0); // |D5| = 10
    }

    /// Canonical form is idempotent.
    #[test]
    fn canonical_form_idempotent() {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        let (canon1, _) = canonical_form(&g);
        let (canon2, _) = canonical_form(&canon1);
        assert_eq!(canon1, canon2);
    }

    /// K5 has a unique canonical form regardless of labeling.
    #[test]
    fn canonical_form_k5() {
        // K5 with identity labeling
        let mut g1 = AdjacencyMatrix::new(5);
        for i in 0..5 {
            for j in (i + 1)..5 {
                g1.set_edge(i, j, true);
            }
        }

        // K5 is the same under any permutation
        let g2 = g1.permute_vertices(&[4, 3, 2, 1, 0]);

        let (canon1, aut1) = canonical_form(&g1);
        let (canon2, aut2) = canonical_form(&g2);

        assert_eq!(canon1, canon2);
        assert_eq!(aut1, 120.0); // |S5| = 120
        assert_eq!(aut2, 120.0);
    }

    /// Canonical form of empty graph on 0 vertices.
    #[test]
    fn canonical_form_empty() {
        let g = AdjacencyMatrix::new(0);
        let (canon, aut) = canonical_form(&g);
        assert_eq!(canon.n(), 0);
        assert_eq!(aut, 1.0);
    }

    /// canonical_form aut_order matches standalone automorphism_group_order.
    #[test]
    fn canonical_form_aut_matches() {
        let mut g = AdjacencyMatrix::new(8);
        for i in 0..8 {
            g.set_edge(i, (i + 1) % 8, true);
            g.set_edge(i, (i + 4) % 8, true);
        }
        let standalone = automorphism_group_order(&g);
        let (_, from_canon) = canonical_form(&g);
        assert_eq!(standalone, from_canon);
    }

    /// Non-isomorphic graphs produce different canonical forms.
    #[test]
    fn canonical_form_non_isomorphic() {
        // C5 (5-cycle)
        let mut c5 = AdjacencyMatrix::new(5);
        for i in 0..5 {
            c5.set_edge(i, (i + 1) % 5, true);
        }
        // P5 (path on 5 vertices: 0-1-2-3-4)
        let mut p5 = AdjacencyMatrix::new(5);
        p5.set_edge(0, 1, true);
        p5.set_edge(1, 2, true);
        p5.set_edge(2, 3, true);
        p5.set_edge(3, 4, true);

        let (canon_c5, _) = canonical_form(&c5);
        let (canon_p5, _) = canonical_form(&p5);

        assert_ne!(canon_c5, canon_p5, "C5 and P5 are not isomorphic");
    }
}
