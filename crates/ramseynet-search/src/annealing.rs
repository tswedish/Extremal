use rand::rngs::SmallRng;
use rand::Rng;
use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::Verdict;
use ramseynet_verifier::verify_ramsey;

use crate::search::{SearchResult, Searcher};
use crate::viz::SearchObserver;

/// Simulated annealing: random edge flips with a cooling schedule.
/// Accepts worsening moves with probability exp(-delta/temp).
pub struct AnnealingSearcher {
    pub initial_temp: f64,
    pub cooling_rate: f64,
}

impl Default for AnnealingSearcher {
    fn default() -> Self {
        Self {
            initial_temp: 2.0,
            cooling_rate: 0.9995,
        }
    }
}

/// Count the number of violations (clique + independent set witnesses).
/// Returns 0 if the graph is Ramsey-valid.
fn violation_score(graph: &AdjacencyMatrix, k: u32, ell: u32) -> u32 {
    let cid = compute_cid(graph);
    let result = verify_ramsey(graph, k, ell, &cid);
    match result.verdict {
        Verdict::Accepted => 0,
        Verdict::Rejected => {
            // Count both clique and independent set violations
            let mut score = 0;
            if let Some(ref w) = result.witness {
                score += w.len() as u32;
            }
            score.max(1)
        }
    }
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

        // Start with a random graph
        let mut graph = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                if rng.gen_bool(0.5) {
                    graph.set_edge(i, j, true);
                }
            }
        }

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

        for iter in 0..max_iters {
            if iter % 100 == 0 {
                observer.on_progress(
                    &graph, n, k, ell, "annealing",
                    iter, max_iters, false, current_score,
                );
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
                observer.on_progress(
                    &graph, n, k, ell, "annealing",
                    iter + 1, max_iters, true, 0,
                );
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
                }
            } else {
                // Reject — undo
                graph.set_edge(i, j, old_val);
            }

            temp *= self.cooling_rate;
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
        assert!(result.valid, "annealing should find a valid R(3,3) graph on 5 vertices");
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
