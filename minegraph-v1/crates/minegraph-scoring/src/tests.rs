//! Integration tests for the full scoring pipeline.
//!
//! Verifies:
//! - score_bytes ordering matches GraphScore Ord
//! - Canonical CIDs correctly dedup isomorphic graphs
//! - Threshold comparison works correctly
//! - Histogram computation is consistent with clique counting

#[cfg(test)]
mod scoring_pipeline {
    use crate::automorphism::canonical_form;
    use crate::clique::{NeighborSet, count_cliques};
    use crate::goodman::{goodman_gap, goodman_minimum};
    use crate::histogram::CliqueHistogram;
    use crate::score::GraphScore;
    use minegraph_graph::AdjacencyMatrix;

    /// Build a scored graph from a matrix.
    fn score(matrix: &AdjacencyMatrix, max_k: u32) -> (GraphScore, Vec<u8>) {
        let histogram = CliqueHistogram::compute(matrix, max_k);
        let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
        let gap = goodman_gap(matrix.n(), red_tri, blue_tri);
        let (canonical, aut_order) = canonical_form(matrix);
        let cid = minegraph_graph::compute_cid(&canonical);
        let score = GraphScore::new(histogram, gap, aut_order, cid);
        let bytes = score.to_score_bytes(max_k);
        (score, bytes)
    }

    #[test]
    fn score_bytes_ordering_matches_ord() {
        // Generate several graphs and verify byte ordering matches Ord
        let mut graphs: Vec<AdjacencyMatrix> = Vec::new();

        // Empty graph
        graphs.push(AdjacencyMatrix::new(10));

        // Complete graph
        let mut k10 = AdjacencyMatrix::new(10);
        for i in 0..10u32 {
            for j in (i + 1)..10 {
                k10.set_edge(i, j, true);
            }
        }
        graphs.push(k10);

        // C5
        let mut c5 = AdjacencyMatrix::new(5);
        for i in 0..5u32 {
            c5.set_edge(i, (i + 1) % 5, true);
        }
        graphs.push(c5);

        // Petersen graph
        let mut pet = AdjacencyMatrix::new(10);
        for (i, j) in [(0, 1), (1, 2), (2, 3), (3, 4), (0, 4)] {
            pet.set_edge(i, j, true);
        }
        for (i, j) in [(5, 7), (7, 9), (6, 9), (6, 8), (5, 8)] {
            pet.set_edge(i, j, true);
        }
        for (i, j) in [(0, 5), (1, 6), (2, 7), (3, 8), (4, 9)] {
            pet.set_edge(i, j, true);
        }
        graphs.push(pet);

        // Random-ish graphs
        let mut g1 = AdjacencyMatrix::new(8);
        g1.set_edge(0, 1, true);
        g1.set_edge(1, 2, true);
        g1.set_edge(2, 3, true);
        g1.set_edge(3, 4, true);
        g1.set_edge(4, 5, true);
        g1.set_edge(5, 6, true);
        g1.set_edge(6, 7, true);
        graphs.push(g1);

        let mut g2 = AdjacencyMatrix::new(8);
        g2.set_edge(0, 1, true);
        g2.set_edge(0, 2, true);
        g2.set_edge(0, 3, true);
        g2.set_edge(1, 4, true);
        g2.set_edge(2, 5, true);
        g2.set_edge(3, 6, true);
        g2.set_edge(4, 7, true);
        graphs.push(g2);

        let scored: Vec<(GraphScore, Vec<u8>)> = graphs.iter().map(|g| score(g, 5)).collect();

        // Verify: for every pair, Ord comparison matches byte comparison
        for i in 0..scored.len() {
            for j in 0..scored.len() {
                let ord_cmp = scored[i].0.cmp(&scored[j].0);
                let byte_cmp = scored[i].1.cmp(&scored[j].1);
                assert_eq!(
                    ord_cmp, byte_cmp,
                    "score_bytes ordering mismatch for graphs {i} vs {j}: \
                     Ord={ord_cmp:?}, bytes={byte_cmp:?}"
                );
            }
        }
    }

    #[test]
    fn canonical_cid_dedup_isomorphic() {
        // Two isomorphic graphs must produce identical canonical CIDs
        let mut g1 = AdjacencyMatrix::new(6);
        g1.set_edge(0, 1, true);
        g1.set_edge(1, 2, true);
        g1.set_edge(2, 3, true);
        g1.set_edge(3, 4, true);
        g1.set_edge(4, 5, true);
        g1.set_edge(5, 0, true); // C6

        // Relabel: 0->2, 1->4, 2->0, 3->5, 4->1, 5->3
        let g2 = g1.permute_vertices(&[2, 4, 0, 5, 1, 3]);

        let (c1, aut1) = canonical_form(&g1);
        let (c2, aut2) = canonical_form(&g2);

        assert_eq!(c1, c2);
        assert_eq!(aut1, aut2);

        let cid1 = minegraph_graph::compute_cid(&c1);
        let cid2 = minegraph_graph::compute_cid(&c2);
        assert_eq!(cid1, cid2);
    }

    #[test]
    fn canonical_cid_distinct_non_isomorphic() {
        // C6 and P6 are not isomorphic, must get different CIDs
        let mut c6 = AdjacencyMatrix::new(6);
        for i in 0..6u32 {
            c6.set_edge(i, (i + 1) % 6, true);
        }

        let mut p6 = AdjacencyMatrix::new(6);
        for i in 0..5u32 {
            p6.set_edge(i, i + 1, true);
        }

        let (cc, _) = canonical_form(&c6);
        let (cp, _) = canonical_form(&p6);
        let cid_c = minegraph_graph::compute_cid(&cc);
        let cid_p = minegraph_graph::compute_cid(&cp);
        assert_ne!(cid_c, cid_p);
    }

    #[test]
    fn threshold_gate_filters_correctly() {
        // Simulate threshold checking: a graph should be admitted only if
        // its score_bytes are strictly less than the threshold
        let mut g_good = AdjacencyMatrix::new(8);
        g_good.set_edge(0, 1, true);
        g_good.set_edge(1, 2, true);

        let mut g_bad = AdjacencyMatrix::new(8);
        for i in 0..8u32 {
            for j in (i + 1)..8 {
                g_bad.set_edge(i, j, true);
            }
        }

        let (_, bytes_good) = score(&g_good, 5);
        let (_, bytes_bad) = score(&g_bad, 5);

        // g_good should have fewer violations than g_bad (K8 has many cliques)
        assert!(
            bytes_good < bytes_bad,
            "sparse graph should score better than K8"
        );

        // Threshold gate: if threshold = bytes_bad, good passes, bad doesn't
        let threshold = &bytes_bad;
        assert!(
            bytes_good.as_slice() < threshold.as_slice(),
            "good should pass"
        );
        assert!(
            bytes_bad.as_slice() >= threshold.as_slice(),
            "bad should not pass (equal)"
        );
    }

    #[test]
    fn histogram_consistent_with_clique_counting() {
        let mut g = AdjacencyMatrix::new(10);
        // Build a graph with some triangles
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(0, 2, true); // triangle 0-1-2
        g.set_edge(3, 4, true);
        g.set_edge(4, 5, true);
        g.set_edge(3, 5, true); // triangle 3-4-5

        let hist = CliqueHistogram::compute(&g, 5);

        // Manual count
        let adj_nbrs = NeighborSet::from_adj(&g);
        let comp = g.complement();
        let comp_nbrs = NeighborSet::from_adj(&comp);

        let red_3 = count_cliques(&adj_nbrs, 3, 10);
        let blue_3 = count_cliques(&comp_nbrs, 3, 10);

        let tier3 = hist.tier(3).expect("should have k=3 tier");
        assert_eq!(tier3.red, red_3);
        assert_eq!(tier3.blue, blue_3);
        assert_eq!(red_3, 2, "two triangles in graph");
    }

    #[test]
    fn goodman_gap_consistency() {
        // Goodman gap should be 0 for optimal graphs (if they exist)
        // For n < R(3,3) = 6, minimum is 0
        assert_eq!(goodman_minimum(5), 0);

        // For n=6, minimum is 2
        assert_eq!(goodman_minimum(6), 2);

        // Gap = actual - minimum, so gap of 0 means at the minimum
        assert_eq!(goodman_gap(5, 0, 0), 0);
        assert_eq!(goodman_gap(6, 1, 1), 0); // 1+1=2 = minimum
        assert_eq!(goodman_gap(6, 2, 1), 1); // 2+1=3, min=2, gap=1
    }

    #[test]
    fn score_symmetry_invariance() {
        // Swapping red/blue (graph vs complement) should give equal scores
        // because tier_key normalizes with (max, min)
        let mut g = AdjacencyMatrix::new(8);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(0, 2, true);

        let comp = g.complement();

        let (score_g, _bytes_g) = score(&g, 5);
        let (score_c, _bytes_c) = score(&comp, 5);

        // The graphs are complements, so swapping red/blue. The tier_key
        // normalizes, but the CIDs differ so scores won't be exactly equal.
        // However, the tier components (max, min) should match.
        for k in 3..=5 {
            if let (Some(tg), Some(tc)) = (score_g.histogram.tier(k), score_c.histogram.tier(k)) {
                let (max_g, min_g) = if tg.red >= tg.blue {
                    (tg.red, tg.blue)
                } else {
                    (tg.blue, tg.red)
                };
                let (max_c, min_c) = if tc.red >= tc.blue {
                    (tc.red, tc.blue)
                } else {
                    (tc.blue, tc.red)
                };
                assert_eq!(
                    (max_g, min_g),
                    (max_c, min_c),
                    "tier k={k} should be symmetric"
                );
            }
        }
    }
}
