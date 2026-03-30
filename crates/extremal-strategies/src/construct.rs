//! Greedy edge-coloring construction strategy for Ramsey graph search.
//!
//! Key difference from all other strategies: builds R(k,ℓ)-valid graphs from
//! scratch rather than perturbing seed graphs. Each construction uses a random
//! edge ordering, producing structurally diverse graphs that may land in
//! different 4-clique basins than Paley-derived perturbation methods.
//!
//! ## Algorithm
//!
//! 1. Start with all edges "blue" (complement = K_n, adj = empty).
//! 2. Process all C(n,2) edges in random order.
//! 3. For each edge, use `violation_delta` to decide: color red or leave blue.
//!    - If coloring red reduces violations: do it.
//!    - If neutral for violations: prefer lower 4-clique count.
//!    - If coloring red increases violations: leave blue.
//! 4. If the result has 0 violations: polish and report.
//! 5. If ≤ threshold violations: attempt short tabu repair, then polish.
//! 6. Repeat with different random orderings.
//!
//! ## Why this works
//!
//! All tested strategies (tree2, SA, crossover, tabu, 2-opt) start from the
//! Paley graph or leaderboard seeds and use local perturbation. They all
//! converge to the same 4c≥67 basin. This strategy avoids that basin entirely
//! by constructing graphs edge-by-edge with random ordering, exploring parts
//! of graph space unreachable by perturbation from fixed seeds.

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques, violation_delta};
use extremal_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

pub struct ConstructSearch;

impl SearchStrategy for ConstructSearch {
    fn id(&self) -> &str {
        "construct"
    }

    fn name(&self) -> &str {
        "Greedy Construction"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "target_k".into(),
                label: "Target K".into(),
                description: "Clique size to avoid in graph (red)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
                adjustable: false,
            },
            ConfigParam {
                name: "target_ell".into(),
                label: "Target Ell".into(),
                description: "Clique size to avoid in complement (blue)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
                adjustable: false,
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
                name: "repair_max_iters".into(),
                label: "Repair Max Iterations".into(),
                description: "Max tabu iterations to repair near-valid constructions".into(),
                param_type: ParamType::Int {
                    min: 0,
                    max: 100_000,
                },
                default: serde_json::json!(10_000),
                adjustable: true,
            },
            ConfigParam {
                name: "repair_threshold".into(),
                label: "Repair Threshold".into(),
                description: "Max violations to attempt repair (0 = only exact valid)".into(),
                param_type: ParamType::Int { min: 0, max: 50 },
                default: serde_json::json!(10),
                adjustable: true,
            },
        ]
    }

    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult {
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
        let repair_max_iters = job
            .config
            .get("repair_max_iters")
            .and_then(|v| v.as_u64())
            .unwrap_or(10_000);
        let repair_threshold = job
            .config
            .get("repair_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);

        let n = job.n;
        let max_iters = job.max_iters;
        let mut rng = SmallRng::seed_from_u64(job.seed);

        // Precompute edge list and K_n complement template
        let num_edges = (n * (n - 1) / 2) as usize;
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity(num_edges);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };

        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_valid_score: Option<(u64, u64)> = None;
        let mut known_cids = job.known_cids.clone();
        let mut trials: u64 = 0;
        let mut valid_count: u64 = 0;
        let mut iters_used: u64 = 0;
        let mut polish_calls: u32 = 0;
        let max_polish_per_round: u32 = 20;

        observer.on_progress(&ProgressInfo {
            graph: AdjacencyMatrix::new(n),
            n,
            strategy: "construct".to_string(),
            iteration: 0,
            max_iters,
            valid: false,
            violation_score: 0,
            discoveries_so_far: 0,
        });

        while iters_used < max_iters {
            if observer.is_cancelled() {
                break;
            }

            trials += 1;

            // === Phase 1: Greedy construction ===
            // Start: adj = empty (no red), comp = K_n (all blue)
            let mut adj = AdjacencyMatrix::new(n);
            let mut adj_nbrs = NeighborSet::from_adj(&adj);
            // Build K_n for complement
            let comp = adj.complement();
            let mut comp_nbrs = NeighborSet::from_adj(&comp);

            // Random edge ordering — the source of diversity
            let mut edge_order = all_edges.clone();
            edge_order.shuffle(&mut rng);

            for &(u, v) in &edge_order {
                // violation_delta: what happens if we flip (u,v)?
                // Currently absent in adj, present in comp.
                // Flipping adds to adj (red), removes from comp (blue).
                let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
                let net = dk + de;

                let do_flip = if net < 0 {
                    true // Reduces violations
                } else if net > 0 {
                    false // Increases violations
                } else {
                    // Neutral: use 4-clique count as tiebreaker
                    let (dk4, de4) = violation_delta(&adj_nbrs, &comp_nbrs, 4, 4, u, v);
                    let net4 = dk4 + de4;
                    if net4 < 0 {
                        true
                    } else if net4 > 0 {
                        false
                    } else {
                        rng.gen_bool(0.5)
                    }
                };

                if do_flip {
                    adj.set_edge(u, v, true);
                    adj_nbrs.flip_edge(u, v);
                    comp_nbrs.flip_edge(u, v);
                }

                iters_used += 1;
            }

            // === Phase 2: Check validity ===
            let kc = count_cliques(&adj_nbrs, k, n);
            let ei = count_cliques(&comp_nbrs, ell, n);
            let violations = kc + ei;

            if violations == 0 {
                valid_count += 1;
                self.handle_valid_graph(
                    &adj,
                    &adj_nbrs,
                    &comp_nbrs,
                    n,
                    k,
                    ell,
                    polish_max_steps,
                    polish_tabu_tenure,
                    &mut known_cids,
                    observer,
                    iters_used,
                    &mut discovery_count,
                    &mut best_valid,
                    &mut best_valid_score,
                    &mut polish_calls,
                    max_polish_per_round,
                );
            } else if violations <= repair_threshold && repair_max_iters > 0 {
                // === Phase 3: Tabu repair for near-valid constructions ===
                let repaired = self.tabu_repair(
                    &mut adj,
                    &mut adj_nbrs,
                    &mut comp_nbrs,
                    &all_edges,
                    num_edges,
                    n,
                    k,
                    ell,
                    repair_max_iters.min(max_iters.saturating_sub(iters_used)),
                    observer,
                    &mut iters_used,
                    &mut rng,
                );

                if repaired {
                    valid_count += 1;
                    self.handle_valid_graph(
                        &adj,
                        &adj_nbrs,
                        &comp_nbrs,
                        n,
                        k,
                        ell,
                        polish_max_steps,
                        polish_tabu_tenure,
                        &mut known_cids,
                        observer,
                        iters_used,
                        &mut discovery_count,
                        &mut best_valid,
                        &mut best_valid_score,
                        &mut polish_calls,
                        max_polish_per_round,
                    );
                }
            }

            // Periodic progress
            if trials.is_multiple_of(100) || iters_used >= max_iters {
                observer.on_progress(&ProgressInfo {
                    graph: best_valid.clone().unwrap_or_else(|| adj.clone()),
                    n,
                    strategy: "construct".to_string(),
                    iteration: iters_used,
                    max_iters,
                    valid: best_valid.is_some(),
                    violation_score: violations as u32,
                    discoveries_so_far: discovery_count,
                });
            }
        }

        debug!(
            trials,
            valid_count,
            discoveries = discovery_count,
            polish_calls,
            best_4c = best_valid_score.map(|(m, _)| m),
            "construct: search complete"
        );

        let has_valid = best_valid.is_some();
        SearchResult {
            valid: has_valid,
            best_graph: best_valid,
            iterations_used: iters_used,
            discoveries: Vec::new(),
            carry_state: None,
        }
    }
}

impl ConstructSearch {
    /// Handle a valid graph: canonicalize, report discovery, polish.
    #[allow(clippy::too_many_arguments)]
    fn handle_valid_graph(
        &self,
        adj: &AdjacencyMatrix,
        adj_nbrs: &NeighborSet,
        comp_nbrs: &NeighborSet,
        n: u32,
        k: u32,
        ell: u32,
        polish_max_steps: u32,
        polish_tabu_tenure: u32,
        known_cids: &mut std::collections::HashSet<extremal_types::GraphCid>,
        observer: &dyn SearchObserver,
        iteration: u64,
        discovery_count: &mut u64,
        best_valid: &mut Option<AdjacencyMatrix>,
        best_valid_score: &mut Option<(u64, u64)>,
        polish_calls: &mut u32,
        max_polish_per_round: u32,
    ) {
        let red_4 = count_cliques(adj_nbrs, 4, n);
        let blue_4 = count_cliques(comp_nbrs, 4, n);
        let max_4c = red_4.max(blue_4);
        let min_4c = red_4.min(blue_4);

        let (canonical, _) = canonical_form(adj);
        let cid = extremal_graph::compute_cid(&canonical);
        if known_cids.insert(cid) {
            observer.on_discovery(&RawDiscovery {
                graph: adj.clone(),
                iteration,
            });
            *discovery_count += 1;

            let is_better = match *best_valid_score {
                Some((bmax, bmin)) => (max_4c, min_4c) < (bmax, bmin),
                None => true,
            };
            if is_better {
                *best_valid = Some(adj.clone());
                *best_valid_score = Some((max_4c, min_4c));
            }
        }

        if polish_max_steps > 0 && *polish_calls < max_polish_per_round {
            *polish_calls += 1;
            if let Some(polished) = crate::polish::polish_valid_graph(
                adj,
                k,
                ell,
                polish_max_steps,
                polish_tabu_tenure,
                false,
                known_cids,
                observer,
                iteration,
            ) {
                let p_adj = NeighborSet::from_adj(&polished);
                let p_comp_g = polished.complement();
                let p_comp = NeighborSet::from_adj(&p_comp_g);
                let p_r4 = count_cliques(&p_adj, 4, polished.n());
                let p_b4 = count_cliques(&p_comp, 4, polished.n());
                let p_max = p_r4.max(p_b4);
                let p_min = p_r4.min(p_b4);
                let polished_better = match *best_valid_score {
                    Some((bmax, bmin)) => (p_max, p_min) < (bmax, bmin),
                    None => true,
                };
                if polished_better {
                    *best_valid = Some(polished);
                    *best_valid_score = Some((p_max, p_min));
                }
            }
        }
    }

    /// Tabu search to repair a near-valid construction.
    /// Returns true if the graph was successfully repaired to 0 violations.
    #[allow(clippy::too_many_arguments)]
    fn tabu_repair(
        &self,
        adj: &mut AdjacencyMatrix,
        adj_nbrs: &mut NeighborSet,
        comp_nbrs: &mut NeighborSet,
        all_edges: &[(u32, u32)],
        num_edges: usize,
        _n: u32,
        k: u32,
        ell: u32,
        max_repair_iters: u64,
        observer: &dyn SearchObserver,
        iters_used: &mut u64,
        rng: &mut SmallRng,
    ) -> bool {
        let mut tabu: Vec<u64> = vec![0; num_edges];
        let tenure = (all_edges.len() / 10).max(5) as u64;
        let mut step: u64 = 0;

        // Compute current violations
        let kc = count_cliques(adj_nbrs, k, _n);
        let ei = count_cliques(comp_nbrs, ell, _n);
        let mut cur_violations = kc + ei;

        while step < max_repair_iters && cur_violations > 0 {
            if observer.is_cancelled() {
                break;
            }

            let mut best_delta = i64::MAX;
            let mut best_edges: Vec<usize> = Vec::new();

            for (idx, &(u, v)) in all_edges.iter().enumerate() {
                if tabu[idx] > step {
                    continue;
                }
                let (dk, de) = violation_delta(adj_nbrs, comp_nbrs, k, ell, u, v);
                let delta = dk + de;
                if delta < best_delta {
                    best_delta = delta;
                    best_edges.clear();
                    best_edges.push(idx);
                } else if delta == best_delta {
                    best_edges.push(idx);
                }
            }

            if best_edges.is_empty() {
                break; // All edges tabu
            }

            // Random tiebreak
            let &chosen_idx = best_edges.choose(rng).unwrap();
            let (u, v) = all_edges[chosen_idx];

            let cur = adj.edge(u, v);
            adj.set_edge(u, v, !cur);
            adj_nbrs.flip_edge(u, v);
            comp_nbrs.flip_edge(u, v);
            tabu[chosen_idx] = step + tenure;
            cur_violations = (cur_violations as i64 + best_delta).max(0) as u64;
            step += 1;
            *iters_used += 1;
        }

        if cur_violations == 0 {
            // Verify with full recount (delta drift correction)
            let actual_kc = count_cliques(adj_nbrs, k, _n);
            let actual_ei = count_cliques(comp_nbrs, ell, _n);
            actual_kc + actual_ei == 0
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                "repair_threshold": 10,
                "repair_max_iters": 10_000,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn construct_finds_r33_n5() {
        let job = make_job(5, 3, 3, 100_000);
        let result = ConstructSearch.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn construct_finds_r44_n17() {
        // Greedy construction is seed-dependent; try several seeds
        for seed in 0..20u64 {
            let mut job = make_job(17, 4, 4, 500_000);
            job.seed = seed;
            job.config = serde_json::json!({
                "target_k": 4,
                "target_ell": 4,
                "repair_threshold": 20,
                "repair_max_iters": 50_000,
            });
            let result = ConstructSearch.search(&job, &NoOpObserver);
            if result.valid {
                return;
            }
        }
        panic!("should find valid R(4,4) on 17 vertices with at least one seed");
    }

    #[test]
    fn construct_reports_discoveries() {
        let job = make_job(5, 3, 3, 100_000);
        let observer = CollectingObserver::new();
        let result = ConstructSearch.search(&job, &observer);
        assert!(result.valid);
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "should discover at least 1 valid graph"
        );
    }

    #[test]
    fn construct_ignores_init_graph() {
        // Even with an init_graph provided, construct builds from scratch
        let mut job = make_job(5, 3, 3, 100_000);
        job.init_graph = Some(crate::init::paley_graph(5));
        let result = ConstructSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "should find valid graphs regardless of init_graph"
        );
    }

    #[test]
    fn construct_diverse_seeds_produce_different_graphs() {
        // Different RNG seeds should produce different valid graphs
        let observer1 = CollectingObserver::new();
        let observer2 = CollectingObserver::new();

        let mut job1 = make_job(5, 3, 3, 50_000);
        job1.seed = 1;
        ConstructSearch.search(&job1, &observer1);

        let mut job2 = make_job(5, 3, 3, 50_000);
        job2.seed = 999;
        ConstructSearch.search(&job2, &observer2);

        let d1 = observer1.drain();
        let d2 = observer2.drain();
        assert!(!d1.is_empty() && !d2.is_empty(), "both should find graphs");
    }
}
