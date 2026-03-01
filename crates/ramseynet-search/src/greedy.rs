use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::Rng;
use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_verifier::verify_ramsey;

use crate::search::{SearchResult, Searcher};
use crate::viz::SearchObserver;

/// Greedy construction: start from a random edge density, shuffle candidate edges,
/// greedily toggle each edge to maintain validity.
pub struct GreedySearcher;

impl Searcher for GreedySearcher {
    fn search(
        &self,
        n: u32,
        k: u32,
        ell: u32,
        max_iters: u64,
        rng: &mut SmallRng,
        observer: &dyn SearchObserver,
    ) -> SearchResult {
        let mut best = AdjacencyMatrix::new(n);
        let mut best_valid = false;
        let mut total_iters = 0u64;

        for _ in 0..max_iters {
            let result = greedy_once(n, k, ell, rng);
            total_iters += 1;

            observer.on_progress(
                &result.graph, n, k, ell, "greedy",
                total_iters, max_iters, result.valid, 0,
            );

            if result.valid {
                return SearchResult {
                    graph: result.graph,
                    valid: true,
                    iterations: total_iters,
                };
            }

            if result.graph.num_edges() > best.num_edges() || !best_valid {
                best = result.graph;
                best_valid = result.valid;
            }
        }

        SearchResult {
            graph: best,
            valid: best_valid,
            iterations: total_iters,
        }
    }

    fn name(&self) -> &'static str {
        "greedy"
    }
}

fn greedy_once(n: u32, k: u32, ell: u32, rng: &mut SmallRng) -> SearchResult {
    // Start with a random graph at ~50% density
    let mut graph = AdjacencyMatrix::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            if rng.gen_bool(0.5) {
                graph.set_edge(i, j, true);
            }
        }
    }

    // Generate all possible edges and shuffle
    let mut edges: Vec<(u32, u32)> = Vec::with_capacity((n * (n - 1) / 2) as usize);
    for i in 0..n {
        for j in (i + 1)..n {
            edges.push((i, j));
        }
    }
    edges.shuffle(rng);

    // For each edge, try toggling it. Keep the toggle if it results in a valid graph.
    for (i, j) in edges {
        let current = graph.edge(i, j);
        graph.set_edge(i, j, !current);
        let cid = compute_cid(&graph);
        let result = verify_ramsey(&graph, k, ell, &cid);
        if result.verdict == ramseynet_types::Verdict::Rejected {
            // Toggling made it invalid — undo
            graph.set_edge(i, j, current);
        }
    }

    // Final check
    let cid = compute_cid(&graph);
    let result = verify_ramsey(&graph, k, ell, &cid);
    let valid = result.verdict == ramseynet_types::Verdict::Accepted;

    SearchResult {
        graph,
        valid,
        iterations: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viz::NoOpObserver;
    use rand::SeedableRng;

    #[test]
    fn greedy_finds_valid_r33_n5() {
        // R(3,3) = 6, so valid graphs exist for n=5
        let searcher = GreedySearcher;
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(5, 3, 3, 100, &mut rng, &NoOpObserver);
        assert!(result.valid, "greedy should find a valid R(3,3) graph on 5 vertices");
        assert_eq!(result.graph.n(), 5);
    }

    #[test]
    fn greedy_fails_r33_n6() {
        // R(3,3) = 6, so no valid graph exists for n=6
        let searcher = GreedySearcher;
        let mut rng = SmallRng::seed_from_u64(42);
        let result = searcher.search(6, 3, 3, 50, &mut rng, &NoOpObserver);
        assert!(!result.valid, "no valid R(3,3) graph exists on 6 vertices");
    }
}
