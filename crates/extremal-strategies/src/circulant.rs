//! Circulant graph enumeration strategy for Ramsey graph search.
//!
//! Exhaustively enumerates all circulant graphs C(n, S) where S ⊆ {1, ..., ⌊n/2⌋}.
//! Circulant graphs are vertex-transitive with |Aut(G)| ≥ n, providing a
//! direct scoring advantage on the 1/|Aut| leaderboard dimension.
//!
//! ## Algorithm
//!
//! 1. Enumerate all 2^⌊n/2⌋ connection sets (4096 for n=25).
//! 2. Build circulant graph for each set, check R(k,ℓ) validity.
//! 3. Polish and report any valid graphs.
//! 4. Subsequent rounds: skip (exhaustive search already complete).
//!
//! ## Why this approach
//!
//! All tested heuristic strategies (tree2, crossover, SA, tabu, 2-opt) explore
//! neighborhoods reachable from Paley-derived seeds via local perturbation and
//! are exhausted at the 4c≥67 barrier. Circulant enumeration is:
//! - **Exhaustive** over a structured algebraic subspace (definitive answer)
//! - **Algebraically different** from Paley perturbation (different graph family)
//! - **High symmetry** (aut ≥ n) → direct scoring advantage
//! - **Fast** (~4096 checks for n=25, each O(n^k) ≈ milliseconds)

use extremal_graph::AdjacencyMatrix;
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{NeighborSet, count_cliques};
use extremal_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::{debug, info};

pub struct CirculantSearch;

impl SearchStrategy for CirculantSearch {
    fn id(&self) -> &str {
        "circulant"
    }

    fn name(&self) -> &str {
        "Circulant Enumeration"
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

        let n = job.n;
        let half = n / 2;
        let total_sets = 1u32 << half;

        // Check carry_state — if we already enumerated, skip
        if let Some(ref state) = job.carry_state
            && state.downcast_ref::<bool>().copied().unwrap_or(false)
        {
            debug!("circulant: exhaustive enumeration already complete, skipping");
            return SearchResult {
                valid: false,
                best_graph: None,
                iterations_used: 0,
                discoveries: Vec::new(),
                carry_state: Some(Box::new(true)),
            };
        }

        info!(
            n,
            half, total_sets, "circulant: beginning exhaustive enumeration"
        );

        let mut known_cids = job.known_cids.clone();
        let mut discovery_count: u64 = 0;
        let mut valid_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_valid_score: Option<(u64, u64)> = None;
        let mut polish_calls: u32 = 0;
        let max_polish_per_round: u32 = 50;

        observer.on_progress(&ProgressInfo {
            graph: AdjacencyMatrix::new(n),
            n,
            strategy: "circulant".to_string(),
            iteration: 0,
            max_iters: total_sets as u64,
            valid: false,
            violation_score: 0,
            discoveries_so_far: 0,
        });

        for mask in 0..total_sets {
            if observer.is_cancelled() {
                break;
            }

            // Build circulant graph C(n, S) where S = bits set in mask
            let adj = build_circulant(n, half, mask);
            let adj_nbrs = NeighborSet::from_adj(&adj);
            let comp = adj.complement();
            let comp_nbrs = NeighborSet::from_adj(&comp);

            // Check R(k,ℓ) validity
            let kc = count_cliques(&adj_nbrs, k, n);
            let ei = count_cliques(&comp_nbrs, ell, n);

            if kc + ei == 0 {
                valid_count += 1;

                let red_4 = count_cliques(&adj_nbrs, 4, n);
                let blue_4 = count_cliques(&comp_nbrs, 4, n);
                let max_4c = red_4.max(blue_4);
                let min_4c = red_4.min(blue_4);

                // Canonicalize for CID dedup
                let (canonical, _) = canonical_form(&adj);
                let cid = extremal_graph::compute_cid(&canonical);

                if known_cids.insert(cid) {
                    observer.on_discovery(&RawDiscovery {
                        graph: adj.clone(),
                        iteration: mask as u64,
                    });
                    discovery_count += 1;

                    let is_better = match best_valid_score {
                        Some((bmax, bmin)) => (max_4c, min_4c) < (bmax, bmin),
                        None => true,
                    };
                    if is_better {
                        best_valid = Some(adj.clone());
                        best_valid_score = Some((max_4c, min_4c));
                        info!(
                            mask,
                            red_4, blue_4, max_4c, min_4c, "circulant: new best valid graph"
                        );
                    }

                    // Polish for score improvement
                    if polish_max_steps > 0 && polish_calls < max_polish_per_round {
                        polish_calls += 1;
                        if let Some(polished) = crate::polish::polish_valid_graph(
                            &adj,
                            k,
                            ell,
                            polish_max_steps,
                            polish_tabu_tenure,
                            false,
                            &mut known_cids,
                            observer,
                            mask as u64,
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
                }

                debug!(
                    mask,
                    red_4, blue_4, valid_count, "circulant: valid R({k},{ell}) graph found"
                );
            }

            // Periodic progress
            if mask % 256 == 0 || mask == total_sets - 1 {
                observer.on_progress(&ProgressInfo {
                    graph: best_valid.clone().unwrap_or_else(|| adj.clone()),
                    n,
                    strategy: "circulant".to_string(),
                    iteration: mask as u64,
                    max_iters: total_sets as u64,
                    valid: best_valid.is_some(),
                    violation_score: (kc + ei) as u32,
                    discoveries_so_far: discovery_count,
                });
            }
        }

        info!(
            valid_count,
            discovery_count,
            polish_calls,
            best_4c = ?best_valid_score.map(|(m, _)| m),
            "circulant: enumeration complete"
        );

        SearchResult {
            valid: best_valid.is_some(),
            best_graph: best_valid,
            iterations_used: total_sets as u64,
            discoveries: Vec::new(),
            carry_state: Some(Box::new(true)),
        }
    }
}

/// Build a circulant graph C(n, S) where S is encoded as a bitmask.
/// Bit i (0-indexed) corresponds to connection distance i+1.
fn build_circulant(n: u32, half: u32, mask: u32) -> AdjacencyMatrix {
    let mut g = AdjacencyMatrix::new(n);
    for i in 0..n {
        for bit in 0..half {
            if mask & (1 << bit) != 0 {
                let dist = bit + 1;
                let j = (i + dist) % n;
                if i != j {
                    g.set_edge(i.min(j), i.max(j), true);
                }
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use extremal_worker_api::{CollectingObserver, NoOpObserver};
    use std::collections::HashSet;

    fn make_job(n: u32, k: u32, ell: u32) -> SearchJob {
        SearchJob {
            n,
            max_iters: 1_000_000,
            seed: 42,
            init_graph: None,
            config: serde_json::json!({
                "target_k": k,
                "target_ell": ell,
                "polish_max_steps": 0,
            }),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn build_circulant_c5_cycle() {
        // C(5, {1}) = 5-cycle: each vertex connected to +1 and -1
        let g = build_circulant(5, 2, 0b01); // bit 0 = distance 1
        assert_eq!(g.n(), 5);
        assert_eq!(g.num_edges(), 5);
        for v in 0..5 {
            assert_eq!(g.degree(v), 2);
        }
    }

    #[test]
    fn build_circulant_c5_complete() {
        // C(5, {1,2}) = K5: each vertex connected to all others
        let g = build_circulant(5, 2, 0b11); // bits 0,1 = distances 1,2
        assert_eq!(g.num_edges(), 10);
        for v in 0..5 {
            assert_eq!(g.degree(v), 4);
        }
    }

    #[test]
    fn build_circulant_empty() {
        // C(5, {}) = empty graph
        let g = build_circulant(5, 2, 0b00);
        assert_eq!(g.num_edges(), 0);
    }

    #[test]
    fn circulant_finds_r33_n5() {
        let job = make_job(5, 3, 3);
        let observer = CollectingObserver::new();
        let result = CirculantSearch.search(&job, &observer);
        assert!(
            result.valid,
            "should find valid R(3,3) circulant on 5 vertices"
        );
        let discoveries = observer.drain();
        assert!(
            !discoveries.is_empty(),
            "should discover at least 1 valid circulant"
        );
    }

    #[test]
    fn circulant_finds_r44_n17() {
        // Paley(17) is a circulant graph, so circulant enumeration should find
        // R(4,4)-valid graphs on 17 vertices.
        let job = make_job(17, 4, 4);
        let result = CirculantSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "should find valid R(4,4) circulant on 17 vertices"
        );
    }

    #[test]
    fn circulant_skips_second_round() {
        let mut job = make_job(5, 3, 3);
        // Simulate carry_state from a completed first round
        job.carry_state = Some(Box::new(true));
        let result = CirculantSearch.search(&job, &NoOpObserver);
        assert_eq!(result.iterations_used, 0, "should skip when already done");
    }

    #[test]
    fn circulant_reports_carry_state() {
        let job = make_job(5, 3, 3);
        let result = CirculantSearch.search(&job, &NoOpObserver);
        let state = result.carry_state.expect("should return carry_state");
        assert_eq!(state.downcast_ref::<bool>(), Some(&true));
    }

    #[test]
    fn circulant_deduplicates_isomorphic() {
        // C(5, {1}) and C(5, {2}) are isomorphic (both are C5).
        // Canonical form should dedup them to one discovery.
        let job = make_job(5, 3, 3);
        let observer = CollectingObserver::new();
        CirculantSearch.search(&job, &observer);
        let discoveries = observer.drain();
        // C5 and its complement are both R(3,3)-valid and isomorphic,
        // so we should see exactly 1 unique discovery
        assert_eq!(
            discoveries.len(),
            1,
            "isomorphic circulants should dedup to 1"
        );
    }
}
