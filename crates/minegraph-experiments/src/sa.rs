//! Simulated annealing strategy for Ramsey graph search.
//!
//! A simple SA implementation as a template for contributors.
//! Flips random edges, accepts improvements always and worsening moves
//! with probability e^(-delta/temperature).

use minegraph_graph::AdjacencyMatrix;
use minegraph_scoring::automorphism::canonical_form;
use minegraph_scoring::clique::{NeighborSet, count_cliques, violation_delta};
use minegraph_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use std::collections::HashSet;

pub struct SimulatedAnnealing;

impl SearchStrategy for SimulatedAnnealing {
    fn id(&self) -> &str {
        "sa"
    }

    fn name(&self) -> &str {
        "Simulated Annealing"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "initial_temp".into(),
                label: "Initial Temperature".into(),
                description: "Starting temperature for annealing schedule".into(),
                param_type: ParamType::Float {
                    min: 0.01,
                    max: 100.0,
                },
                default: serde_json::json!(10.0),
                adjustable: true,
            },
            ConfigParam {
                name: "cooling_rate".into(),
                label: "Cooling Rate".into(),
                description: "Multiplicative cooling factor per iteration".into(),
                param_type: ParamType::Float {
                    min: 0.9,
                    max: 0.99999,
                },
                default: serde_json::json!(0.9999),
                adjustable: true,
            },
            ConfigParam {
                name: "target_k".into(),
                label: "Target K".into(),
                description: "Clique size to minimize in graph (red)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
                adjustable: false,
            },
            ConfigParam {
                name: "target_ell".into(),
                label: "Target Ell".into(),
                description: "Clique size to minimize in complement (blue)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
                adjustable: false,
            },
        ]
    }

    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult {
        let initial_temp = job
            .config
            .get("initial_temp")
            .and_then(|v| v.as_f64())
            .unwrap_or(10.0);
        let cooling_rate = job
            .config
            .get("cooling_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.9999);
        let k = job
            .config
            .get("target_k")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u32;
        let ell = job
            .config
            .get("target_ell")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u32;

        let n = job.n;
        let mut rng = SmallRng::seed_from_u64(job.seed);

        // Initialize graph
        let mut graph = job
            .init_graph
            .clone()
            .unwrap_or_else(|| minegraph_strategies::init::random_graph(n, &mut rng));
        let mut comp = graph.complement();
        let mut adj_nbrs = NeighborSet::from_adj(&graph);
        let mut comp_nbrs = NeighborSet::from_adj(&comp);
        let mut violations = count_cliques(&adj_nbrs, k, n) + count_cliques(&comp_nbrs, ell, n);

        let mut best_graph: Option<AdjacencyMatrix> = None;
        let mut best_violations = violations;
        let mut discoveries: Vec<RawDiscovery> = Vec::new();
        let mut known_cids: HashSet<minegraph_types::GraphCid> = job.known_cids.clone();
        let mut temperature = initial_temp;

        // Build edge list
        let edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };

        // Check if seed graph is already valid
        if violations == 0 {
            let (canonical, _) = canonical_form(&graph);
            let cid = minegraph_graph::compute_cid(&canonical);
            if known_cids.insert(cid) {
                let discovery = RawDiscovery {
                    graph: graph.clone(),
                    iteration: 0,
                };
                observer.on_discovery(&discovery);
                discoveries.push(discovery);
                best_graph = Some(graph.clone());
            }
        }

        for iter in 0..job.max_iters {
            if observer.is_cancelled() {
                break;
            }

            // Pick random edge
            let &(u, v) = &edges[rng.gen_range(0..edges.len())];

            // Compute violation delta (returns (delta_k, delta_ell))
            let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
            let delta = dk + de;

            // SA acceptance criterion
            let accept = if delta <= 0 {
                true
            } else {
                let prob = (-delta as f64 / temperature).exp();
                rng.gen_range(0.0..1.0_f64) < prob
            };

            if accept {
                // Apply flip
                let cur = graph.edge(u, v);
                graph.set_edge(u, v, !cur);
                comp.set_edge(u, v, cur);
                adj_nbrs.flip_edge(u, v);
                comp_nbrs.flip_edge(u, v);
                violations = (violations as i64 + delta) as u64;

                if violations < best_violations {
                    best_violations = violations;
                    best_graph = Some(graph.clone());
                }

                // Check for valid graph (zero violations)
                if violations == 0 {
                    let (canonical, _) = canonical_form(&graph);
                    let cid = minegraph_graph::compute_cid(&canonical);
                    if known_cids.insert(cid) {
                        let discovery = RawDiscovery {
                            graph: graph.clone(),
                            iteration: iter,
                        };
                        observer.on_discovery(&discovery);
                        discoveries.push(discovery);
                    }
                }
            }

            // Cool down
            temperature *= cooling_rate;

            // Report progress periodically
            if iter % 1000 == 0 {
                observer.on_progress(&ProgressInfo {
                    graph: graph.clone(),
                    n,
                    strategy: "sa".into(),
                    iteration: iter,
                    max_iters: job.max_iters,
                    valid: violations == 0,
                    violation_score: violations as u32,
                    discoveries_so_far: discoveries.len() as u64,
                });
            }
        }

        SearchResult {
            best_graph,
            valid: !discoveries.is_empty(),
            iterations_used: job.max_iters,
            discoveries,
            carry_state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minegraph_worker_api::CollectingObserver;
    use std::collections::HashSet;

    #[test]
    fn sa_finds_r33_n5() {
        let strategy = SimulatedAnnealing;
        let observer = CollectingObserver::new();
        let job = SearchJob {
            n: 5,
            max_iters: 50_000,
            seed: 42,
            init_graph: Some(minegraph_strategies::init::paley_graph(5)),
            config: serde_json::json!({"target_k": 3, "target_ell": 3, "initial_temp": 5.0}),
            known_cids: HashSet::new(),
            max_known_cids: 1000,
            carry_state: None,
        };

        let result = strategy.search(&job, &observer);
        assert!(result.valid, "SA should find valid R(3,3)/n=5 graph");
    }

    #[test]
    fn sa_finds_r44_n17() {
        let strategy = SimulatedAnnealing;
        let observer = CollectingObserver::new();
        let job = SearchJob {
            n: 17,
            max_iters: 500_000,
            seed: 42,
            init_graph: Some(minegraph_strategies::init::paley_graph(17)),
            config: serde_json::json!({"target_k": 4, "target_ell": 4, "initial_temp": 10.0}),
            known_cids: HashSet::new(),
            max_known_cids: 1000,
            carry_state: None,
        };

        let result = strategy.search(&job, &observer);
        assert!(result.valid, "SA should find valid R(4,4)/n=17 graph");
    }
}
