//! Beam search over single-edge flips for Ramsey graph discovery.
//!
//! # Burst pattern in violation reduction
//!
//! When monitoring the search progress, violation counts tend to decrease in
//! discrete bursts rather than smoothly. This is **expected behavior** for beam
//! search over combinatorial landscapes:
//!
//! - The beam maintains diversity across many candidate graphs at a given
//!   violation level, exploring the neighborhood of the current best.
//! - When a single-edge mutation breaks through to a lower violation count,
//!   the entire beam contracts toward that new basin — candidates that don't
//!   share the breakthrough structure are evicted.
//! - This represents a genuine **phase transition** in the search landscape:
//!   the search has found a structural change that reduces violations, and
//!   the beam re-centers around it.

use std::collections::HashSet;

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_types::GraphCid;
use ramseynet_verifier::clique::count_cliques;
use ramseynet_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};

/// Tree/beam search strategy.
///
/// Systematically explores the neighborhood of a seed graph by maintaining
/// a beam of the best candidates at each depth level. Uses CID-based
/// deduplication to avoid revisiting the same graph.
pub struct TreeSearch;

impl SearchStrategy for TreeSearch {
    fn id(&self) -> &str {
        "tree"
    }

    fn name(&self) -> &str {
        "Tree/Beam Search"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "beam_width".into(),
                label: "Beam Width".into(),
                description: "Number of candidates maintained at each depth level".into(),
                param_type: ParamType::Int { min: 1, max: 10000 },
                default: serde_json::json!(100),
            },
            ConfigParam {
                name: "max_depth".into(),
                label: "Max Depth".into(),
                description:
                    "Maximum depth levels to explore (each level tries all single-edge flips)"
                        .into(),
                param_type: ParamType::Int { min: 1, max: 100 },
                default: serde_json::json!(10),
            },
        ]
    }

    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult {
        // Read config with defaults
        let beam_width = job
            .config
            .get("beam_width")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as usize;
        let max_depth = job
            .config
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;

        let n = job.n;
        let k = job.k;
        let ell = job.ell;
        let max_iters = job.max_iters;

        let mut rng = SmallRng::seed_from_u64(job.seed);

        // Build list of all edges
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };
        let num_edges = all_edges.len();

        // Use platform-provided seed graph or fall back to random
        let seed = job
            .init_graph
            .clone()
            .unwrap_or_else(|| random_graph(n, &mut rng));
        let seed_cid = compute_cid(&seed);
        let (seed_score, seed_kc, seed_ei) = beam_score_detail(&seed, k, ell);

        // Pre-seed with known canonical CIDs from the platform
        let mut seen: HashSet<GraphCid> = job.known_cids.clone();
        seen.insert(seed_cid);

        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64, u64, u64)> =
            Some((seed.clone(), seed_score, seed_kc, seed_ei));
        let mut iters_used: u64 = 1;
        let mut discoveries: Vec<RawDiscovery> = Vec::new();

        if seed_score == 0 {
            best_valid = Some(seed.clone());
            discoveries.push(RawDiscovery {
                graph: seed.clone(),
                iteration: iters_used,
            });
        }

        observer.on_progress(&ProgressInfo {
            graph: seed.clone(),
            n,
            k,
            ell,
            strategy: "tree".to_string(),
            iteration: iters_used,
            max_iters,
            valid: seed_score == 0,
            violation_score: seed_score as u32,
            discoveries_so_far: discoveries.len() as u64,
            k_cliques: Some(seed_kc),
            ell_indsets: Some(seed_ei),
        });

        // Current beam
        let mut beam: Vec<(AdjacencyMatrix, u64)> = vec![(seed, seed_score)];

        for _depth in 0..max_depth {
            if iters_used >= max_iters || beam.is_empty() || observer.is_cancelled() {
                break;
            }

            let remaining = max_iters.saturating_sub(iters_used);
            let full_budget = num_edges as u64 * beam.len() as u64;

            let edges_per_parent = if full_budget > remaining {
                let per = remaining / beam.len().max(1) as u64;
                (per as usize).max(1).min(num_edges)
            } else {
                num_edges
            };

            let mut candidates: Vec<(AdjacencyMatrix, u64)> = Vec::new();

            for (parent, _parent_score) in &beam {
                if iters_used >= max_iters || observer.is_cancelled() {
                    break;
                }

                let edges_to_try: Vec<(u32, u32)> = if edges_per_parent < num_edges {
                    let mut shuffled = all_edges.clone();
                    let (selected, _) = shuffled.partial_shuffle(&mut rng, edges_per_parent);
                    selected.to_vec()
                } else {
                    all_edges.clone()
                };

                for &(i, j) in &edges_to_try {
                    if iters_used >= max_iters || observer.is_cancelled() {
                        break;
                    }

                    let mut child = parent.clone();
                    let current = child.edge(i, j);
                    child.set_edge(i, j, !current);

                    let cid = compute_cid(&child);
                    if !seen.insert(cid) {
                        continue;
                    }

                    let (score, kc, ei) = beam_score_detail(&child, k, ell);
                    iters_used += 1;

                    if score == 0 {
                        // Valid graph found — stream to platform immediately
                        let raw = RawDiscovery {
                            graph: child.clone(),
                            iteration: iters_used,
                        };
                        observer.on_discovery(&raw);
                        discoveries.push(raw);
                        observer.on_progress(&ProgressInfo {
                            graph: child.clone(),
                            n,
                            k,
                            ell,
                            strategy: "tree".to_string(),
                            iteration: iters_used,
                            max_iters,
                            valid: true,
                            violation_score: 0,
                            discoveries_so_far: discoveries.len() as u64,
                            k_cliques: Some(0),
                            ell_indsets: Some(0),
                        });
                        best_valid = Some(child.clone());
                    }

                    if let Some((_, best_inv_score, _, _)) = &best_invalid {
                        if score < *best_inv_score {
                            best_invalid = Some((child.clone(), score, kc, ei));
                        }
                    }

                    candidates.push((child, score));

                    if iters_used.is_multiple_of(100) {
                        let (display_graph, display_score, display_kc, display_ei) =
                            if let Some(ref v) = best_valid {
                                (v, 0u64, 0u64, 0u64)
                            } else if let Some((ref inv, s, kc, ei)) = best_invalid {
                                (inv, s, kc, ei)
                            } else {
                                (&candidates.last().unwrap().0, score, kc, ei)
                            };
                        observer.on_progress(&ProgressInfo {
                            graph: display_graph.clone(),
                            n,
                            k,
                            ell,
                            strategy: "tree".to_string(),
                            iteration: iters_used,
                            max_iters,
                            valid: best_valid.is_some(),
                            violation_score: display_score as u32,
                            discoveries_so_far: discoveries.len() as u64,
                            k_cliques: Some(display_kc),
                            ell_indsets: Some(display_ei),
                        });
                    }
                }
            }

            if candidates.is_empty() {
                break;
            }

            candidates.sort_by_key(|(_, s)| *s);
            candidates.truncate(beam_width);
            beam = candidates;
        }

        let has_valid = best_valid.is_some();
        let best = best_valid.or(best_invalid.map(|(g, _, _, _)| g));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: iters_used,
            discoveries,
        }
    }
}

/// Score a graph by counting violations.
fn beam_score_detail(graph: &AdjacencyMatrix, k: u32, ell: u32) -> (u64, u64, u64) {
    let kc = count_cliques(graph, k);
    let ei = count_cliques(&graph.complement(), ell);
    (kc + ei, kc, ei)
}

/// Simple random graph (50% edge density).
fn random_graph(n: u32, rng: &mut SmallRng) -> AdjacencyMatrix {
    use rand::Rng;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ramseynet_worker_api::observer::NoOpObserver;

    fn make_job(k: u32, ell: u32, n: u32, max_iters: u64) -> SearchJob {
        SearchJob {
            k,
            ell,
            n,
            max_iters,
            seed: 42,
            init_graph: None, // strategy uses random fallback; Paley seed comes from platform
            config: serde_json::json!({}),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
        }
    }

    /// Helper to make a Paley graph for testing (same as init.rs but local)
    fn paley_graph(n: u32) -> AdjacencyMatrix {
        let p = {
            let mut p = n.max(5);
            loop {
                if p % 4 == 1 && is_prime(p) {
                    break p;
                }
                p += 1;
            }
        };
        let mut qr = vec![false; p as usize];
        for x in 1..p {
            qr[((x as u64 * x as u64) % p as u64) as usize] = true;
        }
        let mut g = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                let diff = ((i as i64 - j as i64).rem_euclid(p as i64)) as u32;
                if qr[diff as usize] {
                    g.set_edge(i, j, true);
                }
            }
        }
        g
    }

    fn is_prime(n: u32) -> bool {
        if n < 2 {
            return false;
        }
        if n < 4 {
            return true;
        }
        if n.is_multiple_of(2) || n.is_multiple_of(3) {
            return false;
        }
        let mut i = 5;
        while i * i <= n {
            if n.is_multiple_of(i) || n.is_multiple_of(i + 2) {
                return false;
            }
            i += 6;
        }
        true
    }

    #[test]
    fn tree_finds_valid_r33_n5() {
        let mut job = make_job(3, 3, 5, 10_000);
        // Provide Paley(5) = C5 as seed — should be immediately valid
        job.init_graph = Some(paley_graph(5));
        let result = TreeSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "tree search should find a valid R(3,3) graph on 5 vertices"
        );
        assert!(result.best_graph.is_some());
        assert_eq!(result.best_graph.unwrap().n(), 5);
        assert!(!result.discoveries.is_empty());
    }

    #[test]
    fn tree_fails_r33_n6() {
        let mut job = make_job(3, 3, 6, 10_000);
        job.config = serde_json::json!({"beam_width": 50, "max_depth": 5});
        job.init_graph = Some(paley_graph(6));
        let result = TreeSearch.search(&job, &NoOpObserver);
        assert!(!result.valid, "no valid R(3,3) graph exists on 6 vertices");
    }

    #[test]
    fn tree_finds_valid_r44_n17() {
        let mut job = make_job(4, 4, 17, 100_000);
        // Provide Paley(17) as seed
        job.init_graph = Some(paley_graph(17));
        let result = TreeSearch.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "tree search should find a valid R(4,4) graph on 17 vertices"
        );
        assert_eq!(result.best_graph.unwrap().n(), 17);
    }

    #[test]
    fn tree_respects_budget() {
        let max_iters = 500u64;
        let mut job = make_job(4, 4, 10, max_iters);
        job.config = serde_json::json!({"beam_width": 100, "max_depth": 20});
        job.init_graph = Some(paley_graph(10));
        let result = TreeSearch.search(&job, &NoOpObserver);
        assert!(
            result.iterations_used <= max_iters,
            "iterations {} should not exceed budget {}",
            result.iterations_used,
            max_iters
        );
    }
}
