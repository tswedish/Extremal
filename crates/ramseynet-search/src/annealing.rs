use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::Verdict;
use ramseynet_verifier::verify_ramsey;
use rand::rngs::SmallRng;
use rand::Rng;

use crate::init::{init_graph, InitStrategy};
use crate::search::{SearchResult, Searcher};
use crate::viz::{ProgressInfo, SearchObserver};

/// Simulated annealing: random edge flips with a cooling schedule.
/// Accepts worsening moves with probability exp(-delta/temp).
/// Auto-tunes cooling rate so temperature reaches ~0.01 at max_iters,
/// and restarts from a fresh graph when frozen with no improvement.
pub struct AnnealingSearcher {
    pub initial_temp: f64,
    pub cooling_rate: f64,
    pub init_strategy: InitStrategy,
}

/// Minimum temperature before triggering a restart.
const MIN_TEMP: f64 = 0.01;

/// Iterations at min temp without improvement before restarting.
const RESTART_PATIENCE: u64 = 1000;

impl Default for AnnealingSearcher {
    fn default() -> Self {
        Self {
            initial_temp: 2.0,
            cooling_rate: 0.9995,
            init_strategy: InitStrategy::default(),
        }
    }
}

/// Count violations more granularly: check for both k-clique and ell-independent
/// set, summing witness sizes from both. This gives the optimizer a meaningful
/// gradient to follow.
fn violation_score(graph: &AdjacencyMatrix, k: u32, ell: u32) -> u32 {
    let cid = compute_cid(graph);

    let mut score = 0u32;

    // Check for k-clique in G
    let result = verify_ramsey(graph, k, ell, &cid);
    if result.verdict == Verdict::Accepted {
        return 0;
    }

    if let Some(ref w) = result.witness {
        score += w.len() as u32;
    }

    // Also check the other direction to count both types of violations.
    // verify_ramsey stops at the first violation, so check the complement too.
    let comp = graph.complement();
    if let Some(ref reason) = result.reason {
        if reason == "clique_found" {
            // Already counted clique; now check for independent set too
            if ramseynet_verifier::clique::find_clique_witness(&comp, ell).is_some() {
                score += ell;
            }
        } else {
            // Already counted indep set; now check for clique too
            if ramseynet_verifier::clique::find_clique_witness(graph, k).is_some() {
                score += k;
            }
        }
    }

    score.max(1)
}

impl Searcher for AnnealingSearcher {
    fn search(
        &self,
        n: u32,
        k: u32,
        ell: u32,
        max_iters: u64,
        rng: &mut SmallRng,
        observer: &dyn SearchObserver,
    ) -> SearchResult {
        if n < 2 {
            let g = AdjacencyMatrix::new(n);
            return SearchResult {
                graph: g,
                valid: true,
                iterations: 0,
            };
        }

        // Auto-tune cooling rate so temp reaches MIN_TEMP at max_iters.
        // cooling_rate^max_iters = MIN_TEMP / initial_temp
        // => cooling_rate = (MIN_TEMP / initial_temp)^(1/max_iters)
        let cooling_rate = if self.cooling_rate > 0.0 && max_iters > 0 {
            (MIN_TEMP / self.initial_temp).powf(1.0 / max_iters as f64)
        } else {
            self.cooling_rate
        };

        let mut graph = init_graph(n, &self.init_strategy, rng);
        let mut current_score = violation_score(&graph, k, ell);
        if current_score == 0 {
            return SearchResult {
                graph,
                valid: true,
                iterations: 1,
            };
        }

        let mut best_graph = graph.clone();
        let mut best_score = current_score;
        let mut temp = self.initial_temp;
        let mut iters_since_improvement = 0u64;

        for iter in 0..max_iters {
            if iter % 100 == 0 {
                if observer.is_cancelled() {
                    break;
                }
                observer.on_progress(&ProgressInfo {
                    graph: &graph,
                    n,
                    k,
                    ell,
                    strategy: "annealing",
                    iteration: iter,
                    max_iters,
                    valid: false,
                    violation_score: current_score,
                    k_cliques: None,
                    ell_indsets: None,
                });
            }

            // Restart if frozen and stagnated
            if temp <= MIN_TEMP && iters_since_improvement >= RESTART_PATIENCE {
                graph = init_graph(n, &self.init_strategy, rng);
                current_score = violation_score(&graph, k, ell);
                if current_score == 0 {
                    observer.on_progress(&ProgressInfo {
                        graph: &graph,
                        n,
                        k,
                        ell,
                        strategy: "annealing",
                        iteration: iter + 1,
                        max_iters,
                        valid: true,
                        violation_score: 0,
                        k_cliques: None,
                        ell_indsets: None,
                    });
                    return SearchResult {
                        graph,
                        valid: true,
                        iterations: iter + 1,
                    };
                }
                temp = self.initial_temp;
                iters_since_improvement = 0;
                continue;
            }

            // Random edge flip
            let i = rng.gen_range(0..n);
            let mut j = rng.gen_range(0..n - 1);
            if j >= i {
                j += 1;
            }

            let old_val = graph.edge(i, j);
            graph.set_edge(i, j, !old_val);

            let new_score = violation_score(&graph, k, ell);

            if new_score == 0 {
                observer.on_progress(&ProgressInfo {
                    graph: &graph,
                    n,
                    k,
                    ell,
                    strategy: "annealing",
                    iteration: iter + 1,
                    max_iters,
                    valid: true,
                    violation_score: 0,
                    k_cliques: None,
                    ell_indsets: None,
                });
                return SearchResult {
                    graph,
                    valid: true,
                    iterations: iter + 1,
                };
            }

            let delta = new_score as f64 - current_score as f64;
            if delta <= 0.0 || rng.gen::<f64>() < (-delta / temp).exp() {
                // Accept the move
                current_score = new_score;
                if new_score < best_score {
                    best_graph = graph.clone();
                    best_score = new_score;
                    iters_since_improvement = 0;
                } else {
                    iters_since_improvement += 1;
                }
            } else {
                // Reject — undo
                graph.set_edge(i, j, old_val);
                iters_since_improvement += 1;
            }

            temp *= cooling_rate;
        }

        SearchResult {
            graph: best_graph,
            valid: best_score == 0,
            iterations: max_iters,
        }
    }

    fn name(&self) -> &'static str {
        "annealing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viz::NoOpObserver;
    use rand::SeedableRng;

    #[test]
    fn annealing_finds_valid_r33_n5() {
        let searcher = AnnealingSearcher::default();
        let mut rng = SmallRng::seed_from_u64(99);
        let result = searcher.search(5, 3, 3, 50_000, &mut rng, &NoOpObserver);
        assert!(
            result.valid,
            "annealing should find a valid R(3,3) graph on 5 vertices"
        );
        assert_eq!(result.graph.n(), 5);
    }

    #[test]
    fn annealing_fails_r33_n6() {
        let searcher = AnnealingSearcher::default();
        let mut rng = SmallRng::seed_from_u64(99);
        let result = searcher.search(6, 3, 3, 10_000, &mut rng, &NoOpObserver);
        assert!(!result.valid);
    }
}
