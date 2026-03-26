//! Canonical labeling and automorphism group computation via nauty.
//!
//! Uses `nauty-Traces-sys` (nauty 2.9.3) for:
//! - **Canonical form**: relabel a graph so that isomorphic graphs produce
//!   identical adjacency matrices (and therefore identical CIDs)
//! - **|Aut(G)|**: automorphism group order (used in scoring — higher symmetry
//!   is better)
//!
//! A single `densenauty` call computes both simultaneously.

use std::os::raw::c_int;

use extremal_graph::AdjacencyMatrix;
use nauty_Traces_sys::*;

/// Run nauty's `densenauty` on a graph.
///
/// Returns `(lab, aut_order)` where:
/// - `lab[i]` is the original vertex at position `i` in the canonical labeling
/// - `aut_order` is `|Aut(G)|` as f64
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

/// Compute `|Aut(G)|` using nauty.
///
/// Returns the automorphism group order as f64.
/// For small graphs (n <= 20) this is exact when the order fits in f64.
pub fn automorphism_group_order(adj: &AdjacencyMatrix) -> f64 {
    let (_lab, aut_order) = run_nauty(adj);
    aut_order
}

/// Compute the canonical form of a graph.
///
/// Returns `(canonical_matrix, aut_order)` where:
/// - `canonical_matrix` is the graph relabeled so that isomorphic graphs
///   produce identical adjacency matrices
/// - `aut_order` is `|Aut(G)|` (computed in the same nauty call)
///
/// Two graphs G and H are isomorphic iff
/// `canonical_form(G).0 == canonical_form(H).0`.
///
/// The canonical CID is then `blake3(graph6(canonical_matrix))`.
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

    let canonical = adj.permute_vertices(&inv_lab);
    (canonical, aut_order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_c5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5 {
            g.set_edge(i, (i + 1) % 5, true);
        }
        g
    }

    fn make_k5() -> AdjacencyMatrix {
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5u32 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        g
    }

    #[test]
    fn c5_aut_order() {
        assert_eq!(automorphism_group_order(&make_c5()), 10.0); // |D5| = 10
    }

    #[test]
    fn k5_aut_order() {
        assert_eq!(automorphism_group_order(&make_k5()), 120.0); // |S5| = 120
    }

    #[test]
    fn empty_graph_aut() {
        let g = AdjacencyMatrix::new(5);
        assert_eq!(automorphism_group_order(&g), 120.0); // 5! = 120
    }

    #[test]
    fn single_vertex_aut() {
        assert_eq!(automorphism_group_order(&AdjacencyMatrix::new(1)), 1.0);
    }

    #[test]
    fn zero_vertices_aut() {
        assert_eq!(automorphism_group_order(&AdjacencyMatrix::new(0)), 1.0);
    }

    #[test]
    fn paley17_aut() {
        let mut g = AdjacencyMatrix::new(17);
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
        assert_eq!(automorphism_group_order(&g), 136.0); // 17 * 8
    }

    #[test]
    fn canonical_form_isomorphic_c5() {
        let g1 = make_c5();

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
        assert_eq!(aut1, 10.0);
    }

    #[test]
    fn canonical_form_idempotent() {
        let (canon1, _) = canonical_form(&make_c5());
        let (canon2, _) = canonical_form(&canon1);
        assert_eq!(canon1, canon2);
    }

    #[test]
    fn canonical_form_k5_any_permutation() {
        let g1 = make_k5();
        let g2 = g1.permute_vertices(&[4, 3, 2, 1, 0]);

        let (canon1, aut1) = canonical_form(&g1);
        let (canon2, aut2) = canonical_form(&g2);

        assert_eq!(canon1, canon2);
        assert_eq!(aut1, 120.0);
        assert_eq!(aut2, 120.0);
    }

    #[test]
    fn canonical_form_empty() {
        let (canon, aut) = canonical_form(&AdjacencyMatrix::new(0));
        assert_eq!(canon.n(), 0);
        assert_eq!(aut, 1.0);
    }

    #[test]
    fn canonical_form_non_isomorphic() {
        let c5 = make_c5();
        let mut p5 = AdjacencyMatrix::new(5);
        p5.set_edge(0, 1, true);
        p5.set_edge(1, 2, true);
        p5.set_edge(2, 3, true);
        p5.set_edge(3, 4, true);

        let (canon_c5, _) = canonical_form(&c5);
        let (canon_p5, _) = canonical_form(&p5);
        assert_ne!(canon_c5, canon_p5, "C5 and P5 are not isomorphic");
    }

    #[test]
    fn canonical_cid_dedup() {
        // Two isomorphic graphs must produce the same blake3 CID
        let g1 = make_c5();
        let g2 = g1.permute_vertices(&[2, 0, 3, 1, 4]);

        let (canon1, _) = canonical_form(&g1);
        let (canon2, _) = canonical_form(&g2);

        let cid1 = extremal_graph::compute_cid(&canon1);
        let cid2 = extremal_graph::compute_cid(&canon2);
        assert_eq!(cid1, cid2, "isomorphic graphs must have same canonical CID");
    }

    #[test]
    fn canonical_form_aut_matches_standalone() {
        // Wagner-like graph on 8 vertices
        let mut g = AdjacencyMatrix::new(8);
        for i in 0..8u32 {
            g.set_edge(i, (i + 1) % 8, true);
            g.set_edge(i, (i + 4) % 8, true);
        }
        let standalone = automorphism_group_order(&g);
        let (_, from_canon) = canonical_form(&g);
        assert_eq!(standalone, from_canon);
    }
}
