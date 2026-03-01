use std::collections::HashSet;

use rand::rngs::SmallRng;
use rand::Rng;
use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::Verdict;
use ramseynet_verifier::verify_ramsey;

use crate::search::{SearchResult, Searcher};
use crate::viz::SearchObserver;

/// Local search with tabu: start from a random graph, use witness-directed
/// edge flips to repair violations, with a tabu list to avoid cycles.
pub struct LocalSearcher {
    pub tabu_tenure: u32,
}

impl Default for LocalSearcher {
    fn default() -> Self {
        Self { tabu_tenure: 10 }
    }
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
        // Start with a random graph (each edge present with probability 0.5)
        let mut graph = random_graph(n, rng);
        let mut tabu: HashSet<(u32, u32)> = HashSet::new();
        let mut tabu_queue: Vec<(u32, u32)> = Vec::new();

        for iter in 0..max_iters {
            let cid = compute_cid(&graph);
            let result = verify_ramsey(&graph, k, ell, &cid);

            if result.verdict == Verdict::Accepted {
                observer.on_progress(&graph, n, k, ell, "local", iter + 1, max_iters, true, 0);
                return SearchResult {
                    graph,
                    valid: true,
                    iterations: iter + 1,
                };
            }

            let witness_len = result.witness.as_ref().map(|w| w.len() as u32).unwrap_or(1);

            if iter % 100 == 0 {
                observer.on_progress(
                    &graph, n, k, ell, "local",
                    iter, max_iters, false, witness_len,
                );
            }

            // Use witness to guide repair
            if let Some(witness) = result.witness {
                let flipped = witness_directed_flip(&mut graph, &witness, &result.reason, &tabu, n, rng);
                if let Some(edge) = flipped {
                    // Add to tabu list
                    tabu.insert(edge);
                    tabu_queue.push(edge);
                    // Expire old tabu entries
                    while tabu_queue.len() > self.tabu_tenure as usize {
                        let old = tabu_queue.remove(0);
                        tabu.remove(&old);
                    }
                } else {
                    // No non-tabu flip available — do a random flip
                    random_flip(&mut graph, n, rng);
                }
            } else {
                random_flip(&mut graph, n, rng);
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

fn random_graph(n: u32, rng: &mut SmallRng) -> AdjacencyMatrix {
    let mut g = AdjacencyMatrix::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            if rng.gen_bool(0.5) {
                g.set_edge(i, j, true);
            }
        }
    }
    g
}

/// Flip an edge incident to a witness vertex to try to break the violation.
fn witness_directed_flip(
    graph: &mut AdjacencyMatrix,
    witness: &[u32],
    reason: &Option<String>,
    tabu: &HashSet<(u32, u32)>,
    n: u32,
    rng: &mut SmallRng,
) -> Option<(u32, u32)> {
    let is_clique = reason.as_deref() == Some("clique_found");

    // For a clique violation: try removing an edge between witness vertices
    // For an independent set violation: try adding an edge between witness vertices
    // In both cases, we flip edges that match the violation type
    let mut candidates: Vec<(u32, u32)> = Vec::new();
    for (idx, &v) in witness.iter().enumerate() {
        for &w in &witness[idx + 1..] {
            let (lo, hi) = if v < w { (v, w) } else { (w, v) };
            if tabu.contains(&(lo, hi)) {
                continue;
            }
            // For cliques: flip present edges; for indep sets: flip absent edges
            let dominated = if is_clique { graph.edge(v, w) } else { !graph.edge(v, w) };
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

fn random_flip(graph: &mut AdjacencyMatrix, n: u32, rng: &mut SmallRng) {
    if n < 2 {
        return;
    }
    let i = rng.gen_range(0..n);
    let mut j = rng.gen_range(0..n - 1);
    if j >= i {
        j += 1;
    }
    let current = graph.edge(i, j);
    graph.set_edge(i, j, !current);
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
        assert!(result.valid, "local search should find a valid R(3,3) graph on 5 vertices");
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
