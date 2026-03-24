//! Tabu search strategy for Ramsey graph coloring.
//!
//! Follows a single trajectory through the search space, using a tabu list
//! to prevent cycling. Complementary to tree2's beam search: tree2 explores
//! breadth, tabu explores depth.
//!
//! ## Algorithm
//!
//! 1. Start from seed graph, compute violations (k-cliques + ell-cliques).
//! 2. Each iteration: evaluate all candidate edge flips.
//! 3. Pick the non-tabu flip with the best violation delta.
//! 4. Aspiration: allow tabu moves if they produce a new best-ever score.
//! 5. On reaching zero violations: polish the valid graph, then continue.
//!
//! ## Config parameters
//!
//! - `tabu_tenure` (int, default 50): iterations an edge stays tabu
//! - `focused` (bool, default false): restrict flips to guilty edges
//! - `polish_rounds` (int, default 3): polish iterations per valid graph
//! - `target_k` (int, default 5): clique size in graph
//! - `target_ell` (int, default 5): clique size in complement

use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

use minegraph_graph::AdjacencyMatrix;
use minegraph_scoring::automorphism::canonical_form;
use minegraph_scoring::clique::{NeighborSet, count_cliques, guilty_edges, violation_delta};
use minegraph_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

pub struct TabuSearch;

impl SearchStrategy for TabuSearch {
    fn id(&self) -> &str {
        "tabu"
    }

    fn name(&self) -> &str {
        "Tabu Search"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "tabu_tenure".into(),
                label: "Tabu Tenure".into(),
                description: "Iterations an edge stays tabu after being flipped".into(),
                param_type: ParamType::Int { min: 1, max: 500 },
                default: serde_json::json!(50),
                adjustable: true,
            },
            ConfigParam {
                name: "focused".into(),
                label: "Focused Edges".into(),
                description: "Only flip edges participating in violations".into(),
                param_type: ParamType::Bool,
                default: serde_json::json!(false),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_rounds".into(),
                label: "Polish Rounds".into(),
                description: "Score-optimization rounds per valid graph found".into(),
                param_type: ParamType::Int { min: 0, max: 10 },
                default: serde_json::json!(3),
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
        let tabu_tenure = job
            .config
            .get("tabu_tenure")
            .and_then(|v| v.as_u64())
            .unwrap_or(50);
        let focused = job
            .config
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let polish_rounds = job
            .config
            .get("polish_rounds")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as u32;
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

        // Build all-edges list
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

        let mut kc = count_cliques(&adj_nbrs, k, n);
        let mut ei = count_cliques(&comp_nbrs, ell, n);
        let mut violations = kc + ei;

        // Tabu matrix: tabu_until[edge_index] = iteration when tabu expires
        let edge_count = (n * (n - 1) / 2) as usize;
        let mut tabu_until: Vec<u64> = vec![0; edge_count];

        let mut best_ever_violations = violations;
        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64)> = Some((graph.clone(), violations));
        let mut known_cids = job.known_cids.clone();

        // Check if seed is already valid
        if violations == 0 {
            let (canonical, _) = canonical_form(&graph);
            let cid = minegraph_graph::compute_cid(&canonical);
            if known_cids.insert(cid) {
                observer.on_discovery(&RawDiscovery {
                    graph: graph.clone(),
                    iteration: 0,
                });
                discovery_count += 1;
                best_valid = Some(graph.clone());
            }
            // Polish the seed
            if polish_rounds > 0
                && let Some(polished) = crate::polish::polish_valid_graph(
                    &graph,
                    k,
                    ell,
                    polish_rounds,
                    &mut known_cids,
                    observer,
                    0,
                )
            {
                best_valid = Some(polished);
            }
        }

        // Report initial progress
        observer.on_progress(&ProgressInfo {
            graph: graph.clone(),
            n,
            strategy: "tabu".to_string(),
            iteration: 0,
            max_iters,
            valid: violations == 0,
            violation_score: violations as u32,
            discoveries_so_far: discovery_count,
        });

        // Recount interval to correct accumulated delta drift
        let recount_interval: u64 = 500;

        for iter in 1..=max_iters {
            if observer.is_cancelled() {
                break;
            }

            // Periodic full recount
            if iter % recount_interval == 0 {
                kc = count_cliques(&adj_nbrs, k, n);
                ei = count_cliques(&comp_nbrs, ell, n);
                violations = kc + ei;
            }

            // Choose candidate edges
            let candidates: Vec<(u32, u32)> = if focused && violations > 0 {
                let ge = guilty_edges(&adj_nbrs, &comp_nbrs, k, ell, n);
                if ge.is_empty() { all_edges.clone() } else { ge }
            } else {
                all_edges.clone()
            };

            // Evaluate all candidates, find best non-tabu and best aspiration
            let mut best_move: Option<(u32, u32, i64)> = None; // (u, v, delta)
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
                        _ => {
                            best_move = Some((u, v, delta));
                        }
                    }
                }

                // Aspiration: allow tabu if it produces new best-ever
                if new_violations < best_ever_violations {
                    match &best_aspiration {
                        Some((_, _, best_d)) if delta >= *best_d => {}
                        _ => {
                            best_aspiration = Some((u, v, delta));
                        }
                    }
                }
            }

            // Choose: aspiration wins if it's better than best non-tabu
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
                    // All moves are tabu and none qualify for aspiration.
                    // Pick a random edge to break the deadlock.
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

            // Update tabu
            let edge_idx = edge_index(u, v, n);
            tabu_until[edge_idx] = iter + tabu_tenure;

            // Update best-ever
            if violations < best_ever_violations {
                best_ever_violations = violations;
                debug!(iter, violations, "tabu: new best violations");
            }

            // Track best invalid
            if violations > 0 {
                match &best_invalid {
                    Some((_, bv)) if violations >= *bv => {}
                    _ => {
                        best_invalid = Some((graph.clone(), violations));
                    }
                }
            }

            // Valid graph found!
            if violations == 0 {
                // Full recount to verify (delta drift can cause false positives)
                let actual_kc = count_cliques(&adj_nbrs, k, n);
                let actual_ei = count_cliques(&comp_nbrs, ell, n);

                if actual_kc + actual_ei == 0 {
                    let (canonical, _) = canonical_form(&graph);
                    let cid = minegraph_graph::compute_cid(&canonical);
                    if known_cids.insert(cid) {
                        observer.on_discovery(&RawDiscovery {
                            graph: graph.clone(),
                            iteration: iter,
                        });
                        discovery_count += 1;
                        best_valid = Some(graph.clone());
                    }

                    // Polish
                    if polish_rounds > 0
                        && let Some(polished) = crate::polish::polish_valid_graph(
                            &graph,
                            k,
                            ell,
                            polish_rounds,
                            &mut known_cids,
                            observer,
                            iter,
                        )
                    {
                        best_valid = Some(polished);
                    }
                } else {
                    // Delta drift — correct it
                    kc = actual_kc;
                    ei = actual_ei;
                    violations = kc + ei;
                }
            }

            // Periodic progress
            if iter % 100 == 0 {
                observer.on_progress(&ProgressInfo {
                    graph: graph.clone(),
                    n,
                    strategy: "tabu".to_string(),
                    iteration: iter,
                    max_iters,
                    valid: best_valid.is_some(),
                    violation_score: violations as u32,
                    discoveries_so_far: discovery_count,
                });
            }
        }

        let has_valid = best_valid.is_some();
        let best = best_valid.or(best_invalid.map(|(g, _)| g));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: max_iters,
            discoveries: Vec::new(),
            carry_state: None,
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
    use minegraph_worker_api::{CollectingObserver, NoOpObserver};
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
                "tabu_tenure": n * 2,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn tabu_finds_r33_n5() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let result = TabuSearch.search(&job, &NoOpObserver);
        assert!(result.valid, "tabu should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn tabu_finds_r44_n17() {
        let mut job = make_job(17, 4, 4, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config = serde_json::json!({
            "target_k": 4,
            "target_ell": 4,
            "tabu_tenure": 34,
        });
        let result = TabuSearch.search(&job, &NoOpObserver);
        assert!(result.valid, "tabu should find valid R(4,4) on 17 vertices");
    }

    #[test]
    fn tabu_discovers_multiple_graphs() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let observer = CollectingObserver::new();
        let result = TabuSearch.search(&job, &observer);
        assert!(result.valid);
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "should discover at least 1 valid graph, got {}",
            discoveries.len()
        );
    }

    #[test]
    fn tabu_focused_r33_n5() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        job.config = serde_json::json!({
            "target_k": 3,
            "target_ell": 3,
            "tabu_tenure": 10,
            "focused": true,
        });
        let result = TabuSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "focused tabu should find valid R(3,3) on 5 vertices"
        );
    }
}
