//! Path relinking strategy for escaping exhausted local-search basins.
//!
//! Key idea: systematically walk between pairs of known-good graphs from
//! the leaderboard, discovering valid intermediates along each path. Each
//! step flips one edge that differs between source and target, choosing the
//! move that maintains R(k,ℓ) validity and minimises 4-clique count.
//!
//! ## Why this is different
//!
//! All local-search methods (tree2, tabu, SA, LNS, crossover, 2-opt,
//! construct) explore random or greedy neighborhoods from a single solution.
//! Path relinking explores the *corridor* between two elite solutions —
//! a structured region of graph space that random search never visits.
//!
//! When no validity-preserving move exists, the algorithm allows brief
//! traversal through 1–2 violation states (controlled by `max_violations`),
//! enabling tunnelling between basins connected through the corridor.
//!
//! ## Algorithm
//!
//! 1. Accumulate a pool of diverse seed graphs from the leaderboard.
//! 2. For each pair (A, B), compute the set of differing edges.
//! 3. Walk from A toward B: at each step, flip the diff-edge that
//!    (a) keeps violations ≤ max_violations, and (b) minimises 4-clique
//!    delta among valid moves (ties broken randomly).
//! 4. Report every novel valid intermediate; polish the best ones.
//! 5. Repeat in reverse (B → A) for a different greedy path.

use std::collections::HashSet;

use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques, violation_delta};
use extremal_types::GraphCid;
use extremal_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

pub struct RelinkSearch;

/// Carried state between rounds: pool of endpoint graphs.
#[derive(Clone, Default)]
struct RelinkState {
    pool: Vec<AdjacencyMatrix>,
    pool_cids: HashSet<GraphCid>,
    /// Pair indices already explored (ordered: smaller first).
    explored_pairs: HashSet<(usize, usize)>,
}

impl SearchStrategy for RelinkSearch {
    fn id(&self) -> &str {
        "relink"
    }

    fn name(&self) -> &str {
        "Path Relinking"
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
                name: "pool_capacity".into(),
                label: "Pool Capacity".into(),
                description: "Max endpoint graphs to accumulate for relinking".into(),
                param_type: ParamType::Int { min: 2, max: 200 },
                default: serde_json::json!(50),
                adjustable: true,
            },
            ConfigParam {
                name: "max_violations".into(),
                label: "Max Violations".into(),
                description: "Max violations allowed during path traversal (0 = strict)".into(),
                param_type: ParamType::Int { min: 0, max: 10 },
                default: serde_json::json!(2),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_max_steps".into(),
                label: "Polish Max Steps".into(),
                description: "Max polish steps per valid intermediate".into(),
                param_type: ParamType::Int { min: 0, max: 5_000 },
                default: serde_json::json!(100),
                adjustable: true,
            },
            ConfigParam {
                name: "polish_tabu_tenure".into(),
                label: "Polish Tabu Tenure".into(),
                description: "Edge tabu tenure during polish".into(),
                param_type: ParamType::Int { min: 5, max: 100 },
                default: serde_json::json!(25),
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
        let pool_capacity = job
            .config
            .get("pool_capacity")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;
        let max_violations = job
            .config
            .get("max_violations")
            .and_then(|v| v.as_u64())
            .unwrap_or(2);
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

        let n = job.n;
        let mut rng = SmallRng::seed_from_u64(job.seed);
        let mut known_cids = job.known_cids.clone();
        let mut discovery_count: u64 = 0;
        let mut iters_used: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_valid_score: Option<(u64, u64)> = None;
        let mut polish_calls: u32 = 0;
        let max_polish_per_round: u32 = 20;
        let mut paths_completed: u64 = 0;
        let mut valid_intermediates: u64 = 0;

        // Restore or create state
        let mut state: RelinkState = job
            .carry_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<RelinkState>())
            .cloned()
            .unwrap_or_default();

        // Add init_graph to pool (dedup by canonical CID)
        if let Some(ref g) = job.init_graph
            && state.pool.len() < pool_capacity
        {
            let (canonical, _) = canonical_form(g);
            let cid = extremal_graph::compute_cid(&canonical);
            if state.pool_cids.insert(cid) {
                state.pool.push(g.clone());
                debug!(pool_size = state.pool.len(), "relink: added seed to pool");
            }
        }

        observer.on_progress(&ProgressInfo {
            graph: job
                .init_graph
                .clone()
                .unwrap_or_else(|| AdjacencyMatrix::new(n)),
            n,
            strategy: "relink".to_string(),
            iteration: 0,
            max_iters: job.max_iters,
            valid: false,
            violation_score: 0,
            discoveries_so_far: 0,
        });

        // Need at least 2 pool entries to relink
        if state.pool.len() < 2 {
            debug!(
                pool_size = state.pool.len(),
                "relink: accumulating pool, need ≥2 graphs"
            );
            return SearchResult {
                valid: false,
                best_graph: job.init_graph.clone(),
                iterations_used: 0,
                discoveries: Vec::new(),
                carry_state: Some(Box::new(state)),
            };
        }

        // Generate unexplored pairs
        let pool_len = state.pool.len();
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for i in 0..pool_len {
            for j in (i + 1)..pool_len {
                if !state.explored_pairs.contains(&(i, j)) {
                    pairs.push((i, j));
                }
            }
        }
        pairs.shuffle(&mut rng);

        debug!(
            pool_size = pool_len,
            unexplored_pairs = pairs.len(),
            "relink: starting relinking"
        );

        // Relink each pair (forward + backward)
        for &(i, j) in &pairs {
            if iters_used >= job.max_iters || observer.is_cancelled() {
                break;
            }

            state.explored_pairs.insert((i, j));

            // Compute diff edges between pool[i] and pool[j]
            let a = &state.pool[i];
            let b = &state.pool[j];
            let mut diff_edges: Vec<(u32, u32)> = Vec::new();
            for u in 0..n {
                for v in (u + 1)..n {
                    if a.edge(u, v) != b.edge(u, v) {
                        diff_edges.push((u, v));
                    }
                }
            }

            if diff_edges.is_empty() {
                continue;
            }

            debug!(
                pair = format!("({i},{j})"),
                diff_edges = diff_edges.len(),
                "relink: starting path"
            );

            // Forward: A → B
            let fwd_result = self.relink_path(
                a,
                &diff_edges,
                n,
                k,
                ell,
                max_violations,
                polish_max_steps,
                polish_tabu_tenure,
                &mut known_cids,
                observer,
                &mut iters_used,
                job.max_iters,
                &mut discovery_count,
                &mut best_valid,
                &mut best_valid_score,
                &mut polish_calls,
                max_polish_per_round,
                &mut valid_intermediates,
                &mut rng,
            );

            paths_completed += 1;

            if iters_used >= job.max_iters || observer.is_cancelled() {
                break;
            }

            // Backward: B → A (reverse diff direction)
            let rev_diff: Vec<(u32, u32)> = diff_edges.iter().copied().rev().collect();
            self.relink_path(
                b,
                &rev_diff,
                n,
                k,
                ell,
                max_violations,
                polish_max_steps,
                polish_tabu_tenure,
                &mut known_cids,
                observer,
                &mut iters_used,
                job.max_iters,
                &mut discovery_count,
                &mut best_valid,
                &mut best_valid_score,
                &mut polish_calls,
                max_polish_per_round,
                &mut valid_intermediates,
                &mut rng,
            );

            paths_completed += 1;

            // Add any newly found best valid graph to the pool
            if let Some(ref g) = fwd_result
                && state.pool.len() < pool_capacity
            {
                let (canonical, _) = canonical_form(g);
                let cid = extremal_graph::compute_cid(&canonical);
                if state.pool_cids.insert(cid) {
                    state.pool.push(g.clone());
                }
            }

            // Progress update
            observer.on_progress(&ProgressInfo {
                graph: best_valid
                    .clone()
                    .unwrap_or_else(|| AdjacencyMatrix::new(n)),
                n,
                strategy: "relink".to_string(),
                iteration: iters_used,
                max_iters: job.max_iters,
                valid: best_valid.is_some(),
                violation_score: 0,
                discoveries_so_far: discovery_count,
            });
        }

        // If we exhausted all pairs, reset explored set for next round
        // (pool may have grown, creating new pairs)
        if pairs.iter().all(|p| state.explored_pairs.contains(p)) {
            debug!("relink: all pairs explored, resetting for next round");
            state.explored_pairs.clear();
        }

        debug!(
            paths_completed,
            valid_intermediates,
            discoveries = discovery_count,
            polish_calls,
            pool_size = state.pool.len(),
            best_4c = best_valid_score.map(|(m, _)| m),
            "relink: round complete"
        );

        let has_valid = best_valid.is_some();
        SearchResult {
            valid: has_valid,
            best_graph: best_valid,
            iterations_used: iters_used,
            discoveries: Vec::new(),
            carry_state: Some(Box::new(state)),
        }
    }
}

impl RelinkSearch {
    /// Walk from `source`, flipping each edge in `diff_edges` (toward the
    /// target graph). Returns the best valid intermediate found (if any).
    #[allow(clippy::too_many_arguments)]
    fn relink_path(
        &self,
        source: &AdjacencyMatrix,
        diff_edges: &[(u32, u32)],
        n: u32,
        k: u32,
        ell: u32,
        max_violations: u64,
        polish_max_steps: u32,
        polish_tabu_tenure: u32,
        known_cids: &mut HashSet<GraphCid>,
        observer: &dyn SearchObserver,
        iters_used: &mut u64,
        max_iters: u64,
        discovery_count: &mut u64,
        best_valid: &mut Option<AdjacencyMatrix>,
        best_valid_score: &mut Option<(u64, u64)>,
        polish_calls: &mut u32,
        max_polish_per_round: u32,
        valid_intermediates: &mut u64,
        rng: &mut SmallRng,
    ) -> Option<AdjacencyMatrix> {
        let mut current = source.clone();
        let mut adj_nbrs = NeighborSet::from_adj(&current);
        let comp = current.complement();
        let mut comp_nbrs = NeighborSet::from_adj(&comp);

        // Compute initial violations
        let kc = count_cliques(&adj_nbrs, k, n);
        let ei = count_cliques(&comp_nbrs, ell, n);
        let mut violations = kc + ei;

        // Track which diff edges remain
        let mut remaining: Vec<(u32, u32)> = diff_edges.to_vec();
        remaining.shuffle(rng);

        let mut path_best: Option<AdjacencyMatrix> = None;
        let mut path_best_score: Option<(u64, u64)> = None;
        let mut steps_since_recount: u32 = 0;

        while !remaining.is_empty() && *iters_used < max_iters {
            if observer.is_cancelled() {
                break;
            }

            // Evaluate all remaining diff edges
            let mut best_move: Option<(usize, u32, u32, i64, i64)> = None; // (idx, u, v, net_delta, score_delta)

            for (idx, &(u, v)) in remaining.iter().enumerate() {
                let (dk, de) = violation_delta(&adj_nbrs, &comp_nbrs, k, ell, u, v);
                let net = dk + de;
                let new_viol = (violations as i64 + net).max(0) as u64;
                *iters_used += 1;

                if new_viol > max_violations {
                    continue; // Would exceed violation tolerance
                }

                // For valid-preserving moves, also evaluate 4c delta
                let score_delta = if new_viol == 0 {
                    let (dk4, de4) = violation_delta(&adj_nbrs, &comp_nbrs, 4, 4, u, v);
                    *iters_used += 1;
                    dk4 + de4
                } else {
                    0 // Don't care about score when in violation state
                };

                let is_better = match best_move {
                    None => true,
                    Some((_, _, _, best_net, best_score)) => {
                        // Prefer: (1) fewer violations, (2) lower 4c
                        (net, score_delta) < (best_net, best_score)
                    }
                };

                if is_better {
                    best_move = Some((idx, u, v, net, score_delta));
                }
            }

            let Some((idx, u, v, net_delta, _)) = best_move else {
                break; // Completely stuck — no move within violation tolerance
            };

            // Apply the move
            let was = current.edge(u, v);
            current.set_edge(u, v, !was);
            adj_nbrs.flip_edge(u, v);
            comp_nbrs.flip_edge(u, v);
            violations = (violations as i64 + net_delta).max(0) as u64;
            remaining.swap_remove(idx);
            steps_since_recount += 1;

            // Periodic full recount to correct drift
            if steps_since_recount >= 50 {
                let actual_kc = count_cliques(&adj_nbrs, k, n);
                let actual_ei = count_cliques(&comp_nbrs, ell, n);
                violations = actual_kc + actual_ei;
                steps_since_recount = 0;
            }

            // If valid, report discovery
            if violations == 0 {
                *valid_intermediates += 1;
                self.handle_valid(
                    &current,
                    &adj_nbrs,
                    &comp_nbrs,
                    n,
                    k,
                    ell,
                    polish_max_steps,
                    polish_tabu_tenure,
                    known_cids,
                    observer,
                    *iters_used,
                    discovery_count,
                    best_valid,
                    best_valid_score,
                    polish_calls,
                    max_polish_per_round,
                );

                // Track path-local best
                let adj_n = NeighborSet::from_adj(&current);
                let comp_g = current.complement();
                let comp_n = NeighborSet::from_adj(&comp_g);
                let r4 = count_cliques(&adj_n, 4, n);
                let b4 = count_cliques(&comp_n, 4, n);
                let score = (r4.max(b4), r4.min(b4));
                let is_path_better = match path_best_score {
                    Some(s) => score < s,
                    None => true,
                };
                if is_path_better {
                    path_best = Some(current.clone());
                    path_best_score = Some(score);
                }
            }
        }

        path_best
    }

    /// Canonicalize, dedup, report discovery, and polish a valid graph.
    #[allow(clippy::too_many_arguments)]
    fn handle_valid(
        &self,
        adj: &AdjacencyMatrix,
        adj_nbrs: &NeighborSet,
        comp_nbrs: &NeighborSet,
        n: u32,
        k: u32,
        ell: u32,
        polish_max_steps: u32,
        polish_tabu_tenure: u32,
        known_cids: &mut HashSet<GraphCid>,
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
                "pool_capacity": 50,
                "max_violations": 2,
                "polish_max_steps": 50,
                "polish_tabu_tenure": 10,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    /// Build two non-isomorphic valid R(3,3) graphs on n=4 for testing.
    /// C4 (4 edges) and matching (2 edges) are both triangle-free with
    /// triangle-free complements, so valid R(3,3).
    fn two_r33_graphs() -> (AdjacencyMatrix, AdjacencyMatrix) {
        // C4 cycle: 0-1-2-3-0
        let mut a = AdjacencyMatrix::new(4);
        a.set_edge(0, 1, true);
        a.set_edge(1, 2, true);
        a.set_edge(2, 3, true);
        a.set_edge(3, 0, true);

        // Perfect matching: 0-1, 2-3
        let mut b = AdjacencyMatrix::new(4);
        b.set_edge(0, 1, true);
        b.set_edge(2, 3, true);

        (a, b)
    }

    #[test]
    fn relink_accumulates_pool() {
        let (a, _) = two_r33_graphs();
        let mut job = make_job(4, 3, 3, 100_000);
        job.init_graph = Some(a);

        let result = RelinkSearch.search(&job, &NoOpObserver);
        assert!(result.carry_state.is_some());
        let state = result
            .carry_state
            .as_ref()
            .unwrap()
            .downcast_ref::<RelinkState>();
        assert!(state.is_some());
        assert_eq!(state.unwrap().pool.len(), 1);
    }

    #[test]
    fn relink_starts_with_two_graphs() {
        let (a, b) = two_r33_graphs();

        // Round 1: add graph A
        let mut job = make_job(4, 3, 3, 100_000);
        job.init_graph = Some(a);
        let r1 = RelinkSearch.search(&job, &NoOpObserver);
        assert_eq!(r1.iterations_used, 0); // Can't relink yet

        // Round 2: add graph B (non-isomorphic), now relinking starts
        job.init_graph = Some(b);
        job.carry_state = r1.carry_state;
        job.seed = 99;
        let observer = CollectingObserver::new();
        let r2 = RelinkSearch.search(&job, &observer);
        assert!(r2.iterations_used > 0, "should have done relinking work");
    }

    #[test]
    fn relink_finds_valid_intermediates() {
        let (a, b) = two_r33_graphs();

        // Pre-load pool with both non-isomorphic graphs
        let state = RelinkState {
            pool: vec![a.clone(), b.clone()],
            pool_cids: {
                let mut s = HashSet::new();
                let (ca, _) = canonical_form(&a);
                s.insert(extremal_graph::compute_cid(&ca));
                let (cb, _) = canonical_form(&b);
                s.insert(extremal_graph::compute_cid(&cb));
                s
            },
            explored_pairs: HashSet::new(),
        };

        let mut job = make_job(4, 3, 3, 100_000);
        job.carry_state = Some(Box::new(state));
        job.config = serde_json::json!({
            "target_k": 3,
            "target_ell": 3,
            "max_violations": 2,
            "polish_max_steps": 50,
            "polish_tabu_tenure": 10,
        });

        let observer = CollectingObserver::new();
        let result = RelinkSearch.search(&job, &observer);

        // Should find valid intermediates between C4 and matching
        assert!(result.valid, "should find valid R(3,3) intermediates");
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "should discover valid intermediate graphs"
        );
    }

    #[test]
    fn relink_carry_state_persists_pool() {
        let (a, b) = two_r33_graphs();

        let state = RelinkState {
            pool: vec![a, b],
            pool_cids: HashSet::new(),
            explored_pairs: HashSet::new(),
        };

        let mut job = make_job(4, 3, 3, 100_000);
        job.carry_state = Some(Box::new(state));
        let result = RelinkSearch.search(&job, &NoOpObserver);

        let state2 = result
            .carry_state
            .as_ref()
            .unwrap()
            .downcast_ref::<RelinkState>();
        assert!(state2.is_some());
        assert!(state2.unwrap().pool.len() >= 2);
    }

    #[test]
    fn relink_respects_budget() {
        let (a, b) = two_r33_graphs();
        let state = RelinkState {
            pool: vec![a, b],
            pool_cids: HashSet::new(),
            explored_pairs: HashSet::new(),
        };

        let mut job = make_job(4, 3, 3, 50); // Very small budget
        job.carry_state = Some(Box::new(state));
        let result = RelinkSearch.search(&job, &NoOpObserver);
        assert!(result.iterations_used <= 60); // Allow small overshoot
    }

    #[test]
    fn relink_r44_n17() {
        // Use Paley(17) as one endpoint, need a second valid R(4,4)
        let paley = crate::init::paley_graph(17);

        // Generate a different valid R(4,4) via greedy construction
        let job_c = SearchJob {
            n: 17,
            max_iters: 500_000,
            seed: 7,
            init_graph: None,
            config: serde_json::json!({
                "target_k": 4,
                "target_ell": 4,
                "repair_threshold": 20,
                "repair_max_iters": 50_000,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        };
        let cr = crate::construct::ConstructSearch.search(&job_c, &NoOpObserver);
        if !cr.valid {
            return; // Can't test without a second graph
        }
        let other = cr.best_graph.unwrap();

        let state = RelinkState {
            pool: vec![paley, other],
            pool_cids: HashSet::new(),
            explored_pairs: HashSet::new(),
        };

        let mut job = make_job(17, 4, 4, 1_000_000);
        job.carry_state = Some(Box::new(state));
        job.config = serde_json::json!({
            "target_k": 4,
            "target_ell": 4,
            "max_violations": 5,
            "polish_max_steps": 50,
            "polish_tabu_tenure": 10,
        });

        let result = RelinkSearch.search(&job, &NoOpObserver);
        // Relinking should at least run (may or may not find valid intermediates)
        assert!(
            result.iterations_used > 0,
            "should have done relinking work on n=17"
        );
    }
}
