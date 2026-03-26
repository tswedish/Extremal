//! Score-aware polishing for valid Ramsey graphs.
//!
//! When a strategy finds a valid graph (zero violations), polish explores
//! its single-flip neighborhood to find better-scoring variants. The key
//! insight: among valid R(k,ℓ) graphs, the leaderboard score is dominated
//! by 4-clique counts, then triangle balance. Polish hill-climbs on these
//! secondary metrics.

use std::collections::HashSet;

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques, violation_delta};
use extremal_types::GraphCid;
use extremal_worker_api::{RawDiscovery, SearchObserver};

/// Score tuple for comparing valid graphs: (max_4c, min_4c, max_3c, min_3c).
/// Lower is better (golf-style, matching leaderboard ordering).
type ValidScore = (u64, u64, u64, u64);

fn score_valid_graph(adj_nbrs: &NeighborSet, comp_nbrs: &NeighborSet, n: u32) -> ValidScore {
    let red_4 = count_cliques(adj_nbrs, 4, n);
    let blue_4 = count_cliques(comp_nbrs, 4, n);
    let red_3 = count_cliques(adj_nbrs, 3, n);
    let blue_3 = count_cliques(comp_nbrs, 3, n);
    (
        red_4.max(blue_4),
        red_4.min(blue_4),
        red_3.max(blue_3),
        red_3.min(blue_3),
    )
}

/// Polish a valid graph by hill-climbing on leaderboard score.
///
/// Tries all single-edge flips, keeping those that are still valid and
/// have a better score. Repeats for `max_rounds` rounds. Reports every
/// novel valid graph found along the way.
///
/// Returns the best-scoring valid graph found (or None if no improvement).
pub fn polish_valid_graph(
    graph: &AdjacencyMatrix,
    k: u32,
    ell: u32,
    max_rounds: u32,
    known_cids: &mut HashSet<GraphCid>,
    observer: &dyn SearchObserver,
    iteration: u64,
) -> Option<AdjacencyMatrix> {
    let n = graph.n();
    let mut current = graph.clone();
    let mut current_comp = current.complement();
    let mut adj_nbrs = NeighborSet::from_adj(&current);
    let mut comp_nbrs = NeighborSet::from_adj(&current_comp);
    let mut current_score = score_valid_graph(&adj_nbrs, &comp_nbrs, n);

    let mut improved = false;

    for _round in 0..max_rounds {
        let mut best_flip: Option<(u32, u32, ValidScore)> = None;

        for u in 0..n {
            for v in (u + 1)..n {
                // Quick validity check via violation_delta
                let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);

                // Current graph is valid (0 violations).
                // After flip, violations = 0 + dk + de. Must stay at 0.
                if dk + de != 0 {
                    continue;
                }

                // Flip, score, unflip
                let cur = current.edge(u, v);
                current.set_edge(u, v, !cur);
                current_comp.set_edge(u, v, cur);
                adj_nbrs.flip_edge(u, v);
                comp_nbrs.flip_edge(u, v);

                let flip_score = score_valid_graph(&adj_nbrs, &comp_nbrs, n);

                // Report this valid neighbor if novel
                let (canonical, _) = canonical_form(&current);
                let cid = extremal_graph::compute_cid(&canonical);
                if known_cids.insert(cid) {
                    observer.on_discovery(&RawDiscovery {
                        graph: current.clone(),
                        iteration,
                    });
                }

                // Track best improvement
                if flip_score < current_score {
                    match &best_flip {
                        Some((_, _, best_s)) if flip_score >= *best_s => {}
                        _ => {
                            best_flip = Some((u, v, flip_score));
                        }
                    }
                }

                // Unflip
                current.set_edge(u, v, cur);
                current_comp.set_edge(u, v, !cur);
                adj_nbrs.flip_edge(u, v);
                comp_nbrs.flip_edge(u, v);
            }
        }

        // Apply best flip for this round
        if let Some((u, v, new_score)) = best_flip {
            let cur = current.edge(u, v);
            current.set_edge(u, v, !cur);
            current_comp.set_edge(u, v, cur);
            adj_nbrs.flip_edge(u, v);
            comp_nbrs.flip_edge(u, v);
            current_score = new_score;
            improved = true;
        } else {
            break; // Local optimum reached
        }
    }

    if improved { Some(current) } else { None }
}
