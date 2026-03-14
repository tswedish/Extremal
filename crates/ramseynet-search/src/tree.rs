//! Beam search over single-edge flips for Ramsey graph discovery.
//!
//! # Burst pattern in violation reduction
//!
//! When monitoring the search progress, violation counts tend to decrease in
//! discrete bursts rather than smoothly. This is **expected behavior** for beam
//! search over combinatorial landscapes:
//!
//! - The beam maintains diversity across many candidate graphs at a given
//!   violation level, exploring the neighborhood of the current best.
//! - When a single-edge mutation breaks through to a lower violation count,
//!   the entire beam contracts toward that new basin — candidates that don't
//!   share the breakthrough structure are evicted.
//! - This represents a genuine **phase transition** in the search landscape:
//!   the search has found a structural change that reduces violations, and
//!   the beam re-centers around it.
//!
//! The `beam_width` parameter controls the exploration-exploitation tradeoff:
//! wider beams explore more of the landscape before converging (more time
//! between bursts, but potentially finding better solutions). Narrower beams
//! converge faster but may miss promising regions.
//!
//! The jumpiness is a feature, not a bug — it indicates the search is properly
//! exploring the combinatorial landscape before finding structural breakthroughs.

use std::collections::HashSet;

use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::GraphCid;
use ramseynet_verifier::clique::count_cliques;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

use crate::init::{init_graph, InitStrategy};
use crate::search::{SearchResult, Searcher};
use crate::viz::{ProgressInfo, SearchObserver};

/// Beam search over single-edge flips from an algebraic seed graph.
///
/// Systematically explores the neighborhood of Paley (or other) seed graphs
/// by maintaining a beam of the best candidates at each depth level. Uses
/// CID-based deduplication to avoid revisiting the same graph.
pub struct TreeSearcher {
    pub beam_width: usize,
    pub max_depth: u32,
    pub init_strategy: InitStrategy,
}

impl Default for TreeSearcher {
    fn default() -> Self {
        Self {
            beam_width: 100,
            max_depth: 10,
            init_strategy: InitStrategy::Paley,
        }
    }
}

/// Score a graph by counting violations, returning (total, k_cliques, ell_indsets).
/// A total of 0 means the graph is valid.
fn beam_score_detail(graph: &AdjacencyMatrix, k: u32, ell: u32) -> (u64, u64, u64) {
    let kc = count_cliques(graph, k);
    let ei = count_cliques(&graph.complement(), ell);
    (kc + ei, kc, ei)
}

impl Searcher for TreeSearcher {
    fn search(
        &self,
        n: u32,
        k: u32,
        ell: u32,
        max_iters: u64,
        rng: &mut SmallRng,
        observer: &dyn SearchObserver,
    ) -> SearchResult {
        // Build list of all edges
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };
        let num_edges = all_edges.len();

        // Initialize seed
        let seed = init_graph(n, &self.init_strategy, rng);
        let seed_cid = compute_cid(&seed);
        let (seed_score, seed_kc, seed_ei) = beam_score_detail(&seed, k, ell);

        // Pre-seed with known canonical CIDs from prior rounds/submissions
        // so we never waste iterations evaluating graphs we've already found.
        let mut seen: HashSet<GraphCid> = observer.known_cids();
        seen.insert(seed_cid);

        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64, u64, u64)> =
            Some((seed.clone(), seed_score, seed_kc, seed_ei));
        let mut iters_used: u64 = 1; // counted seed eval

        if seed_score == 0 {
            best_valid = Some(seed.clone());
            observer.on_valid_found(&seed, n, k, ell, "tree", iters_used);
        }

        observer.on_progress(&ProgressInfo {
            graph: &seed,
            n,
            k,
            ell,
            strategy: "tree",
            iteration: iters_used,
            max_iters,
            valid: seed_score == 0,
            violation_score: seed_score as u32,
            k_cliques: Some(seed_kc),
            ell_indsets: Some(seed_ei),
        });

        // Current beam: Vec of (graph, score)
        let mut beam: Vec<(AdjacencyMatrix, u64)> = vec![(seed, seed_score)];

        for _depth in 0..self.max_depth {
            if iters_used >= max_iters || beam.is_empty() || observer.is_cancelled() {
                break;
            }

            let remaining = max_iters.saturating_sub(iters_used);
            let full_budget = num_edges as u64 * beam.len() as u64;

            // Determine how many edges to sample per parent
            let edges_per_parent = if full_budget > remaining {
                // Budget-limited: sample edges
                let per = remaining / beam.len().max(1) as u64;
                (per as usize).max(1).min(num_edges)
            } else {
                num_edges
            };

            let mut candidates: Vec<(AdjacencyMatrix, u64)> = Vec::new();

            for (parent, _parent_score) in &beam {
                if iters_used >= max_iters || observer.is_cancelled() {
                    break;
                }

                // Get edges to try (full or sampled)
                let edges_to_try: Vec<(u32, u32)> = if edges_per_parent < num_edges {
                    let mut shuffled = all_edges.clone();
                    let (selected, _) = shuffled.partial_shuffle(rng, edges_per_parent);
                    selected.to_vec()
                } else {
                    all_edges.clone()
                };

                for &(i, j) in &edges_to_try {
                    if iters_used >= max_iters || observer.is_cancelled() {
                        break;
                    }

                    let mut child = parent.clone();
                    let current = child.edge(i, j);
                    child.set_edge(i, j, !current);

                    let cid = compute_cid(&child);
                    if !seen.insert(cid) {
                        // Already visited
                        continue;
                    }

                    let (score, kc, ei) = beam_score_detail(&child, k, ell);
                    iters_used += 1;

                    if score == 0 {
                        // Valid graph found — submit immediately
                        observer.on_valid_found(&child, n, k, ell, "tree", iters_used);
                        observer.on_progress(&ProgressInfo {
                            graph: &child,
                            n,
                            k,
                            ell,
                            strategy: "tree",
                            iteration: iters_used,
                            max_iters,
                            valid: true,
                            violation_score: 0,
                            k_cliques: Some(0),
                            ell_indsets: Some(0),
                        });
                        best_valid = Some(child.clone());
                    }

                    // Track best invalid
                    if let Some((_, best_inv_score, _, _)) = &best_invalid {
                        if score < *best_inv_score {
                            best_invalid = Some((child.clone(), score, kc, ei));
                        }
                    }

                    candidates.push((child, score));

                    // Throttled progress via observer (every 100 evals to reduce overhead)
                    if iters_used.is_multiple_of(100) {
                        let (display_graph, display_score, display_kc, display_ei) =
                            if let Some(ref v) = best_valid {
                                (v, 0u64, 0u64, 0u64)
                            } else if let Some((ref inv, s, kc, ei)) = best_invalid {
                                (inv, s, kc, ei)
                            } else {
                                (&candidates.last().unwrap().0, score, kc, ei)
                            };
                        observer.on_progress(&ProgressInfo {
                            graph: display_graph,
                            n,
                            k,
                            ell,
                            strategy: "tree",
                            iteration: iters_used,
                            max_iters,
                            valid: best_valid.is_some(),
                            violation_score: display_score as u32,
                            k_cliques: Some(display_kc),
                            ell_indsets: Some(display_ei),
                        });
                    }
                }
            }

            if candidates.is_empty() {
                break;
            }

            // Sort by score ascending, keep top beam_width
            candidates.sort_by_key(|(_, s)| *s);
            candidates.truncate(self.beam_width);
            beam = candidates;
        }

        // Return best valid, or best invalid
        if let Some(graph) = best_valid {
            SearchResult {
                graph,
                valid: true,
                iterations: iters_used,
            }
        } else if let Some((graph, _, _, _)) = best_invalid {
            SearchResult {
                graph,
                valid: false,
                iterations: iters_used,
            }
        } else {
            // Shouldn't happen (seed is always evaluated), but handle gracefully
            SearchResult {
                graph: AdjacencyMatrix::new(n),
                valid: false,
                iterations: iters_used,
            }
        }
    }

    fn name(&self) -> &'static str {
        "tree"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viz::NoOpObserver;
    use rand::SeedableRng;

    #[test]
    fn tree_finds_valid_r33_n5() {
        // Paley(5) = C5 is valid for R(3,3), seed should be immediately valid
        let searcher = TreeSearcher::default();
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(5, 3, 3, 10_000, &mut rng, &NoOpObserver);
        assert!(
            result.valid,
            "tree search should find a valid R(3,3) graph on 5 vertices"
        );
        assert_eq!(result.graph.n(), 5);
    }

    #[test]
    fn tree_fails_r33_n6() {
        // R(3,3) = 6, so no valid graph exists for n=6
        let searcher = TreeSearcher {
            beam_width: 50,
            max_depth: 5,
            init_strategy: InitStrategy::Paley,
        };
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(6, 3, 3, 10_000, &mut rng, &NoOpObserver);
        assert!(!result.valid, "no valid R(3,3) graph exists on 6 vertices");
    }

    #[test]
    fn tree_finds_valid_r44_n17() {
        // Paley(17) is the unique valid R(4,4) graph on 17 vertices
        let searcher = TreeSearcher::default();
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(17, 4, 4, 100_000, &mut rng, &NoOpObserver);
        assert!(
            result.valid,
            "tree search should find a valid R(4,4) graph on 17 vertices"
        );
        assert_eq!(result.graph.n(), 17);
    }

    #[test]
    fn tree_respects_budget() {
        let max_iters = 500u64;
        let searcher = TreeSearcher {
            beam_width: 100,
            max_depth: 20,
            init_strategy: InitStrategy::Paley,
        };
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(10, 4, 4, max_iters, &mut rng, &NoOpObserver);
        assert!(
            result.iterations <= max_iters,
            "iterations {} should not exceed budget {}",
            result.iterations,
            max_iters
        );
    }
}
