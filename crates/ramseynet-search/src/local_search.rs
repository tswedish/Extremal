use std::collections::HashSet;

use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::Verdict;
use ramseynet_verifier::clique::{count_cliques, find_clique_witness};
use ramseynet_verifier::verify_ramsey;
use rand::rngs::SmallRng;
use rand::Rng;

use crate::init::{init_graph, InitStrategy};
use crate::search::{SearchResult, Searcher};
use crate::viz::{ProgressInfo, SearchObserver};

/// Local search with tabu and restarts: start from a random graph, use
/// witness-directed edge flips to repair violations, with a tabu list to
/// avoid cycles. Restarts from a fresh random graph when stagnated.
pub struct LocalSearcher {
    pub tabu_tenure: u32,
    pub init_strategy: InitStrategy,
}

/// Number of iterations without improvement before restarting.
const RESTART_PATIENCE: u64 = 5000;

/// How often to recompute the full violation score (expensive).
const FULL_SCORE_INTERVAL: u64 = 500;

impl Default for LocalSearcher {
    fn default() -> Self {
        Self {
            tabu_tenure: 10,
            init_strategy: InitStrategy::default(),
        }
    }
}

/// Count total violations: number of k-cliques + number of ell-independent sets.
fn violation_count(graph: &AdjacencyMatrix, complement: &AdjacencyMatrix, k: u32, ell: u32) -> u64 {
    count_cliques(graph, k) + count_cliques(complement, ell)
}

impl Searcher for LocalSearcher {
    fn search(
        &self,
        n: u32,
        k: u32,
        ell: u32,
        max_iters: u64,
        rng: &mut SmallRng,
        observer: &dyn SearchObserver,
    ) -> SearchResult {
        let mut graph = init_graph(n, &self.init_strategy, rng);
        let mut complement = graph.complement();
        let mut tabu: HashSet<(u32, u32)> = HashSet::new();
        let mut tabu_queue: Vec<(u32, u32)> = Vec::new();

        let mut best_score = violation_count(&graph, &complement, k, ell);
        let mut iters_since_improvement = 0u64;

        for iter in 0..max_iters {
            if iter % 100 == 0 && observer.is_cancelled() {
                break;
            }

            // Periodically compute full violation score for stagnation detection
            if iter % FULL_SCORE_INTERVAL == 0 {
                let score = violation_count(&graph, &complement, k, ell);
                if score == 0 {
                    observer.on_progress(&ProgressInfo {
                        graph: &graph,
                        n,
                        k,
                        ell,
                        strategy: "local",
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
                if score < best_score {
                    best_score = score;
                    iters_since_improvement = 0;
                }

                observer.on_progress(&ProgressInfo {
                    graph: &graph,
                    n,
                    k,
                    ell,
                    strategy: "local",
                    iteration: iter,
                    max_iters,
                    valid: false,
                    violation_score: score as u32,
                    k_cliques: None,
                    ell_indsets: None,
                });
            }

            iters_since_improvement += 1;

            // Restart if stagnated
            if iters_since_improvement >= RESTART_PATIENCE {
                graph = init_graph(n, &self.init_strategy, rng);
                complement = graph.complement();
                tabu.clear();
                tabu_queue.clear();
                best_score = violation_count(&graph, &complement, k, ell);
                iters_since_improvement = 0;
                continue;
            }

            // Quick check: find one witness to direct the flip.
            // Use maintained complement to avoid recomputing it each iteration.
            let clique_witness = find_clique_witness(&graph, k);
            let (witness, is_clique) = if let Some(w) = clique_witness {
                (w, true)
            } else if let Some(w) = find_clique_witness(&complement, ell) {
                (w, false)
            } else {
                // No violations found — graph is valid!
                observer.on_progress(&ProgressInfo {
                    graph: &graph,
                    n,
                    k,
                    ell,
                    strategy: "local",
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
            };

            // Use witness to guide repair
            let flipped = witness_directed_flip(&mut graph, &witness, is_clique, &tabu, n, rng);
            if let Some((ei, ej)) = flipped {
                // Maintain complement incrementally
                let comp_val = complement.edge(ei, ej);
                complement.set_edge(ei, ej, !comp_val);

                tabu.insert((ei, ej));
                tabu_queue.push((ei, ej));
                while tabu_queue.len() > self.tabu_tenure as usize {
                    let old = tabu_queue.remove(0);
                    tabu.remove(&old);
                }
            } else {
                let (ri, rj) = random_flip(&mut graph, n, rng);
                // Maintain complement incrementally
                let comp_val = complement.edge(ri, rj);
                complement.set_edge(ri, rj, !comp_val);
            }
        }

        // Final check
        let cid = compute_cid(&graph);
        let result = verify_ramsey(&graph, k, ell, &cid);
        let valid = result.verdict == Verdict::Accepted;

        SearchResult {
            graph,
            valid,
            iterations: max_iters,
        }
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

/// Flip an edge incident to a witness vertex to try to break the violation.
fn witness_directed_flip(
    graph: &mut AdjacencyMatrix,
    witness: &[u32],
    is_clique: bool,
    tabu: &HashSet<(u32, u32)>,
    n: u32,
    rng: &mut SmallRng,
) -> Option<(u32, u32)> {
    // For a clique violation: try removing an edge between witness vertices
    // For an independent set violation: try adding an edge between witness vertices
    let mut candidates: Vec<(u32, u32)> = Vec::new();
    for (idx, &v) in witness.iter().enumerate() {
        for &w in &witness[idx + 1..] {
            let (lo, hi) = if v < w { (v, w) } else { (w, v) };
            if tabu.contains(&(lo, hi)) {
                continue;
            }
            let dominated = if is_clique {
                graph.edge(v, w)
            } else {
                !graph.edge(v, w)
            };
            if dominated {
                candidates.push((lo, hi));
            }
        }
    }

    // If no intra-witness candidates, try edges from witness to non-witness
    if candidates.is_empty() {
        let witness_set: HashSet<u32> = witness.iter().copied().collect();
        for &v in witness {
            for u in 0..n {
                if witness_set.contains(&u) {
                    continue;
                }
                let (lo, hi) = if v < u { (v, u) } else { (u, v) };
                if tabu.contains(&(lo, hi)) {
                    continue;
                }
                candidates.push((lo, hi));
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    let &(i, j) = candidates.get(rng.gen_range(0..candidates.len())).unwrap();
    let current = graph.edge(i, j);
    graph.set_edge(i, j, !current);
    Some((i, j))
}

/// Flip a random edge and return the edge that was flipped.
fn random_flip(graph: &mut AdjacencyMatrix, n: u32, rng: &mut SmallRng) -> (u32, u32) {
    if n < 2 {
        return (0, 0);
    }
    let i = rng.gen_range(0..n);
    let mut j = rng.gen_range(0..n - 1);
    if j >= i {
        j += 1;
    }
    let (lo, hi) = if i < j { (i, j) } else { (j, i) };
    let current = graph.edge(lo, hi);
    graph.set_edge(lo, hi, !current);
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viz::NoOpObserver;
    use rand::SeedableRng;

    #[test]
    fn local_search_finds_valid_r33_n5() {
        let searcher = LocalSearcher::default();
        let mut rng = SmallRng::seed_from_u64(123);
        let result = searcher.search(5, 3, 3, 10_000, &mut rng, &NoOpObserver);
        assert!(
            result.valid,
            "local search should find a valid R(3,3) graph on 5 vertices"
        );
        assert_eq!(result.graph.n(), 5);
    }

    #[test]
    fn local_search_fails_r33_n6() {
        let searcher = LocalSearcher::default();
        let mut rng = SmallRng::seed_from_u64(123);
        let result = searcher.search(6, 3, 3, 1_000, &mut rng, &NoOpObserver);
        assert!(!result.valid);
    }
}
