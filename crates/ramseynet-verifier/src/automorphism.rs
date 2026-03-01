//! Automorphism group computation via nauty's `densenauty`.

use std::os::raw::c_int;

use nauty_Traces_sys::*;
use ramseynet_graph::AdjacencyMatrix;

/// Compute |Aut(G)| using nauty's densenauty.
///
/// Returns the automorphism group order as f64 (grpsize1 * 10^grpsize2).
/// For small graphs (n≤20) this is exact when the group order fits in f64.
pub fn automorphism_group_order(adj: &AdjacencyMatrix) -> f64 {
    let n = adj.n() as usize;
    if n == 0 {
        return 1.0;
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
        ..optionblk::default()
    };
    let mut stats = statsblk::default();

    let mut lab = vec![0i32; n];
    let mut ptn = vec![0i32; n];
    let mut orbits = vec![0i32; n];

    let mut g = empty_graph(m, n);

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
            std::ptr::null_mut(),
        );
    }

    stats.grpsize1 * 10f64.powi(stats.grpsize2)
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
        let qr: std::collections::HashSet<u32> =
            [1, 2, 4, 8, 9, 13, 15, 16].into_iter().collect();
        for i in 0..17u32 {
            for j in (i + 1)..17 {
                let diff = if j > i { j - i } else { i - j };
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
}
