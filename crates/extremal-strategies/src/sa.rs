//! Simulated Annealing strategy for Ramsey graph coloring.
//!
//! Key difference from tabu/tree2: SA can traverse through invalid states
//! by probabilistically accepting worsening moves. This allows it to escape
//! local optima basins (e.g. 4c>=67) that constrained methods can't exit.
//!
//! ## Algorithm
//!
//! 1. Start from seed graph, compute composite objective (violations + quality).
//! 2. Each iteration: pick a random edge, compute objective delta.
//! 3. Accept if improving; accept worsening moves with P = exp(-delta / T).
//! 4. Temperature cools linearly: T = T_start * (1 - iter/max_iters).
//! 5. On reaching zero violations: polish the valid graph, continue searching.
//!
//! ## Composite objective
//!
//! `obj = violations * W + max(red_4c, blue_4c) * 100 + min(red_4c, blue_4c)`
//!
//! At high temperature, SA accepts moves that introduce 1-2 violations if they
//! lower 4-clique counts. At low temperature, it converges to valid graphs —
//! potentially in a different quality basin than where tree2/tabu converge.

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::SmallRng;

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques, violation_delta};
use extremal_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

pub struct SimulatedAnnealing;

/// Compute scalar objective from violations and 4-clique counts.
/// Lower is better. Violations are heavily penalized; among valid graphs,
/// lower max(4c) dominates, then lower min(4c).
#[inline]
fn objective(violations: u64, red_4: u64, blue_4: u64, violation_weight: i64) -> i64 {
    violations as i64 * violation_weight + red_4.max(blue_4) as i64 * 100 + red_4.min(blue_4) as i64
}

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
                name: "sa_initial_temp".into(),
                label: "Initial Temperature".into(),
                description: "Starting temperature (higher = more exploration)".into(),
                param_type: ParamType::Float {
                    min: 0.1,
                    max: 100.0,
                },
                default: serde_json::json!(10.0),
                adjustable: true,
            },
            ConfigParam {
                name: "sa_violation_weight".into(),
                label: "Violation Weight".into(),
                description:
                    "Penalty weight per violation in objective (lower = more invalid traversal)"
                        .into(),
                param_type: ParamType::Int { min: 1, max: 1000 },
                default: serde_json::json!(10),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_max_steps".into(),
                label: "Polish Max Steps".into(),
                description: "Maximum steps in score-aware polish per valid graph".into(),
                param_type: ParamType::Int { min: 0, max: 5_000 },
                default: serde_json::json!(100),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_tabu_tenure".into(),
                label: "Polish Tabu Tenure".into(),
                description: "Steps an edge stays tabu during polish".into(),
                param_type: ParamType::Int { min: 5, max: 100 },
                default: serde_json::json!(25),
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
        let sa_temp = job
            .config
            .get("sa_initial_temp")
            .and_then(|v| v.as_f64())
            .unwrap_or(10.0);
        let violation_weight = job
            .config
            .get("sa_violation_weight")
            .and_then(|v| v.as_i64())
            .unwrap_or(10);
        let polish_max_steps = job
            .config
            .get("polish_max_steps")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as u32;
        let polish_tabu_tenure = job
            .config
            .get("polish_tabu_tenure")
            .and_then(|v| v.as_u64())
            .unwrap_or(25) as u32;
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
        let max_iters = job.max_iters;
        let mut rng = SmallRng::seed_from_u64(job.seed);

        // Build edge list for random selection
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };

        // Initialize from seed
        let mut graph = job
            .init_graph
            .clone()
            .unwrap_or_else(|| crate::init::random_graph(n, &mut rng));
        let mut comp = graph.complement();
        let mut adj_nbrs = NeighborSet::from_adj(&graph);
        let mut comp_nbrs = NeighborSet::from_adj(&comp);

        // Compute initial state
        let mut kc = count_cliques(&adj_nbrs, k, n);
        let mut ei = count_cliques(&comp_nbrs, ell, n);
        let mut violations = kc + ei;
        let mut red_4 = count_cliques(&adj_nbrs, 4, n);
        let mut blue_4 = count_cliques(&comp_nbrs, 4, n);

        let mut current_obj = objective(violations, red_4, blue_4, violation_weight);

        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_valid_score: Option<(u64, u64)> = None; // (max_4c, min_4c)
        let mut best_obj = current_obj;
        let mut best_graph = graph.clone();
        let mut known_cids = job.known_cids.clone();
        let mut polish_calls: u32 = 0;
        let max_polish_per_round: u32 = 5;

        // Recount interval to correct accumulated delta drift
        let recount_interval: u64 = 500;

        // Check if seed is already valid
        if violations == 0 {
            let (canonical, _) = canonical_form(&graph);
            let cid = extremal_graph::compute_cid(&canonical);
            if known_cids.insert(cid) {
                observer.on_discovery(&RawDiscovery {
                    graph: graph.clone(),
                    iteration: 0,
                });
                discovery_count += 1;
                best_valid = Some(graph.clone());
                best_valid_score = Some((red_4.max(blue_4), red_4.min(blue_4)));
            }
            // Polish seed
            if polish_max_steps > 0 && polish_calls < max_polish_per_round {
                polish_calls += 1;
                if let Some(polished) = crate::polish::polish_valid_graph(
                    &graph,
                    k,
                    ell,
                    polish_max_steps,
                    polish_tabu_tenure,
                    false,
                    &mut known_cids,
                    observer,
                    0,
                ) {
                    best_valid = Some(polished);
                }
            }
        }

        // Report initial progress
        observer.on_progress(&ProgressInfo {
            graph: graph.clone(),
            n,
            strategy: "sa".to_string(),
            iteration: 0,
            max_iters,
            valid: violations == 0,
            violation_score: violations as u32,
            discoveries_so_far: discovery_count,
        });

        let mut accepted_count: u64 = 0;

        for iter in 1..=max_iters {
            if observer.is_cancelled() {
                break;
            }

            // Periodic full recount to correct drift
            if iter % recount_interval == 0 {
                kc = count_cliques(&adj_nbrs, k, n);
                ei = count_cliques(&comp_nbrs, ell, n);
                violations = kc + ei;
                red_4 = count_cliques(&adj_nbrs, 4, n);
                blue_4 = count_cliques(&comp_nbrs, 4, n);
                current_obj = objective(violations, red_4, blue_4, violation_weight);
            }

            // Temperature: linear cooling
            let progress = iter as f64 / max_iters as f64;
            let temperature = sa_temp * (1.0 - progress);

            // Pick random edge
            let &(u, v) = &all_edges[rng.gen_range(0..all_edges.len())];

            // Compute deltas
            let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
            let (d_red_4, d_blue_4) = violation_delta(&adj_nbrs, &comp_nbrs, 4, 4, u, v);

            let new_violations = (violations as i64 + dk + de).max(0) as u64;
            let new_red_4 = (red_4 as i64 + d_red_4).max(0) as u64;
            let new_blue_4 = (blue_4 as i64 + d_blue_4).max(0) as u64;
            let new_obj = objective(new_violations, new_red_4, new_blue_4, violation_weight);

            let delta = new_obj - current_obj;

            // SA acceptance criterion
            let accept = if delta <= 0 {
                true // Always accept improvements
            } else if temperature > 0.001 {
                let prob = (-delta as f64 / temperature).exp();
                rng.gen_range(0.0..1.0_f64) < prob
            } else {
                false // Temperature effectively zero — only accept improvements
            };

            if accept {
                // Apply the flip
                let cur = graph.edge(u, v);
                graph.set_edge(u, v, !cur);
                comp.set_edge(u, v, cur);
                adj_nbrs.flip_edge(u, v);
                comp_nbrs.flip_edge(u, v);

                violations = new_violations;
                red_4 = new_red_4;
                blue_4 = new_blue_4;
                current_obj = new_obj;
                accepted_count += 1;

                // Track best overall
                if current_obj < best_obj {
                    best_obj = current_obj;
                    best_graph = graph.clone();
                }

                // Valid graph found!
                if violations == 0 {
                    // Full recount to verify (delta drift can cause false positives)
                    let actual_kc = count_cliques(&adj_nbrs, k, n);
                    let actual_ei = count_cliques(&comp_nbrs, ell, n);

                    if actual_kc + actual_ei == 0 {
                        // Recount 4-cliques too
                        red_4 = count_cliques(&adj_nbrs, 4, n);
                        blue_4 = count_cliques(&comp_nbrs, 4, n);
                        current_obj = objective(0, red_4, blue_4, violation_weight);

                        let max_4c = red_4.max(blue_4);
                        let min_4c = red_4.min(blue_4);
                        let is_new_best = match best_valid_score {
                            Some((bmax, bmin)) => (max_4c, min_4c) < (bmax, bmin),
                            None => true,
                        };

                        let (canonical, _) = canonical_form(&graph);
                        let cid = extremal_graph::compute_cid(&canonical);
                        if known_cids.insert(cid) {
                            observer.on_discovery(&RawDiscovery {
                                graph: graph.clone(),
                                iteration: iter,
                            });
                            discovery_count += 1;

                            if is_new_best {
                                best_valid = Some(graph.clone());
                                best_valid_score = Some((max_4c, min_4c));
                            }
                        }

                        // Polish if this is competitive and budget remains
                        if polish_max_steps > 0 && polish_calls < max_polish_per_round {
                            polish_calls += 1;
                            if let Some(polished) = crate::polish::polish_valid_graph(
                                &graph,
                                k,
                                ell,
                                polish_max_steps,
                                polish_tabu_tenure,
                                false,
                                &mut known_cids,
                                observer,
                                iter,
                            ) {
                                let p_adj = NeighborSet::from_adj(&polished);
                                let p_comp_g = polished.complement();
                                let p_comp = NeighborSet::from_adj(&p_comp_g);
                                let p_r4 = count_cliques(&p_adj, 4, polished.n());
                                let p_b4 = count_cliques(&p_comp, 4, polished.n());
                                let p_max = p_r4.max(p_b4);
                                let p_min = p_r4.min(p_b4);
                                let polished_better = match best_valid_score {
                                    Some((bmax, bmin)) => (p_max, p_min) < (bmax, bmin),
                                    None => true,
                                };
                                if polished_better {
                                    best_valid = Some(polished);
                                    best_valid_score = Some((p_max, p_min));
                                }
                            }
                        }
                    } else {
                        // Delta drift — correct counts
                        kc = actual_kc;
                        ei = actual_ei;
                        violations = kc + ei;
                        red_4 = count_cliques(&adj_nbrs, 4, n);
                        blue_4 = count_cliques(&comp_nbrs, 4, n);
                        current_obj = objective(violations, red_4, blue_4, violation_weight);
                    }
                }
            }

            // Periodic progress
            if iter % 1000 == 0 {
                observer.on_progress(&ProgressInfo {
                    graph: graph.clone(),
                    n,
                    strategy: "sa".to_string(),
                    iteration: iter,
                    max_iters,
                    valid: best_valid.is_some(),
                    violation_score: violations as u32,
                    discoveries_so_far: discovery_count,
                });
            }
        }

        let accept_rate = if max_iters > 0 {
            accepted_count as f64 / max_iters as f64
        } else {
            0.0
        };

        debug!(
            discoveries = discovery_count,
            accept_rate = format!("{:.1}%", accept_rate * 100.0),
            polish_calls,
            best_4c = best_valid_score.map(|(m, _)| m),
            "sa: search complete"
        );

        let has_valid = best_valid.is_some();
        let best = best_valid.or(Some(best_graph));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: max_iters,
            discoveries: Vec::new(),
            carry_state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::paley_graph;
    use extremal_worker_api::{CollectingObserver, NoOpObserver};
    use std::collections::HashSet;

    fn make_job(n: u32, k: u32, ell: u32, max_iters: u64) -> SearchJob {
        SearchJob {
            n,
            max_iters,
            seed: 42,
            init_graph: None,
            config: serde_json::json!({
                "target_k": k,
                "target_ell": ell,
                "sa_initial_temp": 10.0,
                "sa_violation_weight": 10,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn sa_finds_r33_n5() {
        let mut job = make_job(5, 3, 3, 100_000);
        job.init_graph = Some(paley_graph(5));
        let result = SimulatedAnnealing.search(&job, &NoOpObserver);
        assert!(result.valid, "SA should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn sa_finds_r44_n17() {
        let mut job = make_job(17, 4, 4, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config = serde_json::json!({
            "target_k": 4,
            "target_ell": 4,
            "sa_initial_temp": 10.0,
            "sa_violation_weight": 10,
        });
        let result = SimulatedAnnealing.search(&job, &NoOpObserver);
        assert!(result.valid, "SA should find valid R(4,4) on 17 vertices");
    }

    #[test]
    fn sa_discovers_multiple_graphs() {
        let mut job = make_job(5, 3, 3, 100_000);
        job.init_graph = Some(paley_graph(5));
        let observer = CollectingObserver::new();
        let result = SimulatedAnnealing.search(&job, &observer);
        assert!(result.valid);
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "SA should discover at least 1 valid graph"
        );
    }

    #[test]
    fn sa_objective_prefers_valid_at_same_quality() {
        // Among graphs with the same 4c counts, valid should always be preferred
        let valid_obj = objective(0, 65, 65, 10);
        let invalid_obj = objective(1, 65, 65, 10);
        assert!(
            valid_obj < invalid_obj,
            "valid graph should have lower objective at same quality"
        );
    }

    #[test]
    fn sa_objective_allows_invalid_traversal() {
        // SA's key feature: with low violation_weight, an invalid graph with
        // much lower 4c can have a BETTER objective than a valid one with high 4c.
        // This enables traversal through invalid space to reach better basins.
        let valid_high_4c = objective(0, 70, 70, 10);
        let invalid_low_4c = objective(1, 60, 60, 10);
        assert!(
            invalid_low_4c < valid_high_4c,
            "SA should be able to favor low-4c invalid graphs for traversal"
        );
    }

    #[test]
    fn sa_objective_prefers_lower_4c() {
        // Among valid graphs, lower max(4c) should win
        let low_4c = objective(0, 65, 65, 10);
        let high_4c = objective(0, 67, 67, 10);
        assert!(low_4c < high_4c, "lower 4c should have lower objective");
    }
}
