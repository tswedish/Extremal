//! Crossover recombination strategy for Ramsey graph search.
//!
//! Combines structural features from two parent graphs via uniform edge
//! crossover, then repairs violations using tabu search. Designed to escape
//! the local optima that beam search (tree2) converges to.
//!
//! ## Algorithm
//!
//! 1. Parent A: leaderboard seed (`init_graph`)
//! 2. Parent B: drawn from a carry-state pool of previously found valid graphs,
//!    or (first round) a perturbed copy of parent A
//! 3. Create offspring via uniform crossover: each edge randomly from A or B
//! 4. Repair offspring violations via tabu search
//! 5. Polish valid offspring (score-aware tabu walk)
//! 6. Accumulate valid graphs into carry-state pool for future crossover
//!
//! ## Config parameters
//!
//! - `crossover_rate` (float, default 0.5): probability of inheriting edge from parent A
//! - `num_offspring` (int, default 10): offspring per round
//! - `tabu_tenure` (int, default 50): tabu tenure during violation repair
//! - `perturb_flips` (int, default 8): random flips to create parent B (first round)
//! - `polish_max_steps` (int, default 100): polish steps per valid graph
//! - `polish_tabu_tenure` (int, default 25): tabu tenure during polish
//! - `target_k` (int, default 5): clique size in graph
//! - `target_ell` (int, default 5): clique size in complement

use std::collections::HashSet;

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques, guilty_edges, violation_delta};
use extremal_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

pub struct CrossoverSearch;

impl SearchStrategy for CrossoverSearch {
    fn id(&self) -> &str {
        "crossover"
    }

    fn name(&self) -> &str {
        "Crossover Recombination"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "crossover_rate".into(),
                label: "Crossover Rate".into(),
                description: "Probability of inheriting each edge from parent A (vs B)".into(),
                param_type: ParamType::Float { min: 0.1, max: 0.9 },
                default: serde_json::json!(0.5),
                adjustable: true,
            },
            ConfigParam {
                name: "num_offspring".into(),
                label: "Offspring Per Round".into(),
                description: "Number of crossover offspring generated per round".into(),
                param_type: ParamType::Int { min: 1, max: 100 },
                default: serde_json::json!(10),
                adjustable: true,
            },
            ConfigParam {
                name: "tabu_tenure".into(),
                label: "Repair Tabu Tenure".into(),
                description: "Iterations an edge stays tabu during violation repair".into(),
                param_type: ParamType::Int { min: 1, max: 500 },
                default: serde_json::json!(50),
                adjustable: true,
            },
            ConfigParam {
                name: "perturb_flips".into(),
                label: "Perturbation Flips".into(),
                description: "Random edge flips to create parent B (when pool empty)".into(),
                param_type: ParamType::Int { min: 1, max: 50 },
                default: serde_json::json!(8),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_max_steps".into(),
                label: "Polish Max Steps".into(),
                description: "Maximum steps in score-aware polish walk".into(),
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
        let crossover_rate = job
            .config
            .get("crossover_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let num_offspring = job
            .config
            .get("num_offspring")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;
        let tabu_tenure = job
            .config
            .get("tabu_tenure")
            .and_then(|v| v.as_u64())
            .unwrap_or(50);
        let perturb_flips = job
            .config
            .get("perturb_flips")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as u32;
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

        // Restore parent pool from carry_state
        let mut pool: Vec<AdjacencyMatrix> = job
            .carry_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<Vec<AdjacencyMatrix>>())
            .cloned()
            .unwrap_or_default();

        // Parent A: leaderboard seed
        let parent_a = job
            .init_graph
            .clone()
            .unwrap_or_else(|| crate::init::random_graph(n, &mut rng));

        // Parent B: from pool (previous valid graphs) or perturbed parent A
        let parent_b = if !pool.is_empty() {
            pool.choose(&mut rng).unwrap().clone()
        } else {
            let mut b = parent_a.clone();
            crate::init::perturb(&mut b, perturb_flips, &mut rng);
            b
        };

        // All edges for tabu repair
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };

        let iters_per_offspring = max_iters / num_offspring.max(1) as u64;
        let mut total_iters: u64 = 0;
        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64)> = None;
        let mut known_cids = job.known_cids.clone();
        let recount_interval: u64 = 500;

        debug!(
            num_offspring,
            iters_per_offspring,
            crossover_rate,
            pool_size = pool.len(),
            "crossover: starting round"
        );

        for _offspring_idx in 0..num_offspring {
            if observer.is_cancelled() {
                break;
            }

            // Create offspring via uniform edge crossover
            let mut graph = AdjacencyMatrix::new(n);
            for u in 0..n {
                for v in (u + 1)..n {
                    let edge = if rng.gen_bool(crossover_rate) {
                        parent_a.edge(u, v)
                    } else {
                        parent_b.edge(u, v)
                    };
                    graph.set_edge(u, v, edge);
                }
            }

            let mut comp = graph.complement();
            let mut adj_nbrs = NeighborSet::from_adj(&graph);
            let mut comp_nbrs = NeighborSet::from_adj(&comp);

            let mut kc = count_cliques(&adj_nbrs, k, n);
            let mut ei = count_cliques(&comp_nbrs, ell, n);
            let mut violations = kc + ei;

            // Handle already-valid offspring (unlikely but possible)
            if violations == 0 {
                handle_valid(
                    &graph,
                    k,
                    ell,
                    polish_max_steps,
                    polish_tabu_tenure,
                    &mut known_cids,
                    observer,
                    total_iters,
                    &mut discovery_count,
                    &mut best_valid,
                    &mut pool,
                );
                total_iters += 1;
                continue;
            }

            // Tabu repair: drive violations to zero
            let edge_count = (n * (n - 1) / 2) as usize;
            let mut tabu_until: Vec<u64> = vec![0; edge_count];
            let mut best_ever_violations = violations;

            for iter in 1..=iters_per_offspring {
                if observer.is_cancelled() {
                    break;
                }

                // Periodic full recount to correct delta drift
                if iter % recount_interval == 0 {
                    kc = count_cliques(&adj_nbrs, k, n);
                    ei = count_cliques(&comp_nbrs, ell, n);
                    violations = kc + ei;
                }

                // Focused candidates when violations are low
                let candidates: Vec<(u32, u32)> = if violations <= 20 {
                    let ge = guilty_edges(&adj_nbrs, &comp_nbrs, k, ell, n);
                    if ge.is_empty() { all_edges.clone() } else { ge }
                } else {
                    all_edges.clone()
                };

                // Evaluate all candidates for best non-tabu and aspiration moves
                let mut best_move: Option<(u32, u32, i64)> = None;
                let mut best_aspiration: Option<(u32, u32, i64)> = None;

                for &(u, v) in &candidates {
                    let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
                    let delta = dk + de;
                    let new_violations = (violations as i64 + delta).max(0) as u64;

                    let edge_idx = edge_index(u, v, n);
                    let is_tabu = tabu_until[edge_idx] > iter;

                    if !is_tabu {
                        match &best_move {
                            Some((_, _, best_d)) if delta >= *best_d => {}
                            _ => best_move = Some((u, v, delta)),
                        }
                    }

                    if new_violations < best_ever_violations {
                        match &best_aspiration {
                            Some((_, _, best_d)) if delta >= *best_d => {}
                            _ => best_aspiration = Some((u, v, delta)),
                        }
                    }
                }

                let chosen = match (best_aspiration, best_move) {
                    (Some(asp), Some(reg)) => {
                        if asp.2 < reg.2 {
                            asp
                        } else {
                            reg
                        }
                    }
                    (Some(asp), None) => asp,
                    (None, Some(reg)) => reg,
                    (None, None) => {
                        let &(u, v) = all_edges.choose(&mut rng).unwrap();
                        let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
                        (u, v, dk + de)
                    }
                };

                let (u, v, delta) = chosen;

                // Apply the flip
                let cur = graph.edge(u, v);
                graph.set_edge(u, v, !cur);
                comp.set_edge(u, v, cur);
                adj_nbrs.flip_edge(u, v);
                comp_nbrs.flip_edge(u, v);

                violations = (violations as i64 + delta).max(0) as u64;

                let edge_idx = edge_index(u, v, n);
                tabu_until[edge_idx] = iter + tabu_tenure;

                if violations < best_ever_violations {
                    best_ever_violations = violations;
                }

                // Track best invalid
                if violations > 0 {
                    match &best_invalid {
                        Some((_, bv)) if violations >= *bv => {}
                        _ => best_invalid = Some((graph.clone(), violations)),
                    }
                }

                // Valid graph found — verify, report, polish
                if violations == 0 {
                    let actual_kc = count_cliques(&adj_nbrs, k, n);
                    let actual_ei = count_cliques(&comp_nbrs, ell, n);

                    if actual_kc + actual_ei == 0 {
                        handle_valid(
                            &graph,
                            k,
                            ell,
                            polish_max_steps,
                            polish_tabu_tenure,
                            &mut known_cids,
                            observer,
                            total_iters + iter,
                            &mut discovery_count,
                            &mut best_valid,
                            &mut pool,
                        );
                    } else {
                        // Delta drift — correct counts
                        kc = actual_kc;
                        ei = actual_ei;
                        violations = kc + ei;
                    }
                }

                // Periodic progress
                if iter % 1000 == 0 {
                    observer.on_progress(&ProgressInfo {
                        graph: graph.clone(),
                        n,
                        strategy: "crossover".to_string(),
                        iteration: total_iters + iter,
                        max_iters,
                        valid: best_valid.is_some(),
                        violation_score: violations as u32,
                        discoveries_so_far: discovery_count,
                    });
                }
            }

            total_iters += iters_per_offspring;
        }

        // Final progress
        observer.on_progress(&ProgressInfo {
            graph: best_valid
                .clone()
                .or(best_invalid.as_ref().map(|(g, _)| g.clone()))
                .unwrap_or_else(|| parent_a.clone()),
            n,
            strategy: "crossover".to_string(),
            iteration: total_iters,
            max_iters,
            valid: best_valid.is_some(),
            violation_score: 0,
            discoveries_so_far: discovery_count,
        });

        debug!(
            discovery_count,
            offspring = num_offspring,
            pool_size = pool.len(),
            "crossover: round complete"
        );

        // Trim pool to bounded size
        const MAX_POOL: usize = 20;
        if pool.len() > MAX_POOL {
            pool.truncate(MAX_POOL);
        }

        let has_valid = best_valid.is_some();
        let best = best_valid.or(best_invalid.map(|(g, _)| g));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: total_iters,
            discoveries: Vec::new(),
            carry_state: Some(Box::new(pool)),
        }
    }
}

/// Handle a valid graph: canonicalize, dedup, report, polish, add to pool.
#[allow(clippy::too_many_arguments)]
fn handle_valid(
    graph: &AdjacencyMatrix,
    k: u32,
    ell: u32,
    polish_max_steps: u32,
    polish_tabu_tenure: u32,
    known_cids: &mut HashSet<extremal_types::GraphCid>,
    observer: &dyn SearchObserver,
    iteration: u64,
    discovery_count: &mut u64,
    best_valid: &mut Option<AdjacencyMatrix>,
    pool: &mut Vec<AdjacencyMatrix>,
) {
    let (canonical, _) = canonical_form(graph);
    let cid = extremal_graph::compute_cid(&canonical);
    if known_cids.insert(cid) {
        observer.on_discovery(&RawDiscovery {
            graph: graph.clone(),
            iteration,
        });
        *discovery_count += 1;
        *best_valid = Some(graph.clone());

        if pool.len() < 20 {
            pool.push(graph.clone());
        }
    }

    // Polish for better score
    if polish_max_steps > 0
        && let Some(polished) = crate::polish::polish_valid_graph(
            graph,
            k,
            ell,
            polish_max_steps,
            polish_tabu_tenure,
            known_cids,
            observer,
            iteration,
        )
    {
        *best_valid = Some(polished.clone());
        if pool.len() < 20 {
            pool.push(polished);
        }
    }
}

/// Map edge (u, v) with u < v to a flat index.
#[inline]
fn edge_index(u: u32, v: u32, n: u32) -> usize {
    let (u, v) = if u < v { (u, v) } else { (v, u) };
    (u * n - u * (u + 1) / 2 + (v - u - 1)) as usize
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
                "num_offspring": 5,
                "tabu_tenure": n * 2,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn crossover_finds_r33_n5() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let result = CrossoverSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "crossover should find valid R(3,3) on 5 vertices"
        );
    }

    #[test]
    fn crossover_finds_r44_n17() {
        let mut job = make_job(17, 4, 4, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config = serde_json::json!({
            "target_k": 4,
            "target_ell": 4,
            "num_offspring": 5,
            "tabu_tenure": 34,
        });
        let result = CrossoverSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "crossover should find valid R(4,4) on 17 vertices"
        );
    }

    #[test]
    fn crossover_discovers_via_observer() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let observer = CollectingObserver::new();
        let result = CrossoverSearch.search(&job, &observer);
        assert!(result.valid);
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "should discover at least 1 valid graph"
        );
    }

    #[test]
    fn crossover_carry_state_builds_pool() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let result = CrossoverSearch.search(&job, &NoOpObserver);

        // carry_state should contain the parent pool
        assert!(result.carry_state.is_some());
        let pool = result
            .carry_state
            .as_ref()
            .unwrap()
            .downcast_ref::<Vec<AdjacencyMatrix>>();
        assert!(pool.is_some(), "carry_state should be Vec<AdjacencyMatrix>");
    }

    #[test]
    fn crossover_uses_pool_from_carry_state() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));

        // First round: builds pool
        let r1 = CrossoverSearch.search(&job, &NoOpObserver);
        assert!(r1.valid);

        // Second round: uses pool as parent B source
        job.carry_state = r1.carry_state;
        job.seed = 99; // different seed
        let r2 = CrossoverSearch.search(&job, &NoOpObserver);
        assert!(r2.valid);
    }
}
