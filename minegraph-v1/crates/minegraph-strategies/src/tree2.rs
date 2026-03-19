//! Incremental beam search over single-edge flips (tree2).
//!
//! Production default strategy, ported from the RamseyNet prototype.
//!
//! Three key optimizations:
//! 1. **Flip-score-unflip**: In-place flip, compute delta, unflip. Zero alloc.
//! 2. **Incremental violation delta**: Bitwise neighbor masks for O(n^k-2)
//!    delta instead of O(n^k) full recount.
//! 3. **64-bit fingerprint dedup**: XOR-fold for beam dedup instead of blake3.
//!
//! ## Config parameters
//!
//! Passed via `SearchJob.config` JSON:
//! - `beam_width` (int, default 100): candidates kept per depth
//! - `max_depth` (int, default 10): depth levels to search
//! - `focused` (bool, default false): only flip violation-participating edges
//! - `target_k` (int, default 5): clique size to minimize in graph
//! - `target_ell` (int, default 5): clique size to minimize in complement

use std::collections::HashSet;
use std::time::Instant;

use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;

use minegraph_graph::AdjacencyMatrix;
use minegraph_scoring::automorphism::canonical_form;
use minegraph_scoring::clique::{
    NeighborSet, count_cliques, fast_fingerprint, guilty_edges, violation_delta,
};
use minegraph_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

/// A beam entry: graph + complement + neighbor bitmasks + violation counts.
struct BeamEntry {
    graph: AdjacencyMatrix,
    comp: AdjacencyMatrix,
    adj_nbrs: NeighborSet,
    comp_nbrs: NeighborSet,
    violations: u64,
    kc: u64,
    ei: u64,
}

impl BeamEntry {
    fn from_graph(graph: AdjacencyMatrix, k: u32, ell: u32) -> Self {
        let comp = graph.complement();
        let adj_nbrs = NeighborSet::from_adj(&graph);
        let comp_nbrs = NeighborSet::from_adj(&comp);
        let n = graph.n();
        let kc = count_cliques(&adj_nbrs, k, n);
        let ei = count_cliques(&comp_nbrs, ell, n);
        BeamEntry {
            graph,
            comp,
            adj_nbrs,
            comp_nbrs,
            violations: kc + ei,
            kc,
            ei,
        }
    }

    /// Flip edge (u,v) in graph, complement, and both neighbor sets.
    #[inline]
    fn flip(&mut self, u: u32, v: u32) {
        let cur = self.graph.edge(u, v);
        self.graph.set_edge(u, v, !cur);
        self.comp.set_edge(u, v, cur);
        self.adj_nbrs.flip_edge(u, v);
        self.comp_nbrs.flip_edge(u, v);
    }
}

/// Incremental beam search strategy.
pub struct Tree2Search;

impl SearchStrategy for Tree2Search {
    fn id(&self) -> &str {
        "tree2"
    }

    fn name(&self) -> &str {
        "Incremental Beam Search"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "beam_width".into(),
                label: "Beam Width".into(),
                description: "Number of candidates kept per depth level".into(),
                param_type: ParamType::Int {
                    min: 1,
                    max: 10_000,
                },
                default: serde_json::json!(100),
            },
            ConfigParam {
                name: "max_depth".into(),
                label: "Max Depth".into(),
                description: "Number of depth levels to search".into(),
                param_type: ParamType::Int { min: 1, max: 100 },
                default: serde_json::json!(10),
            },
            ConfigParam {
                name: "focused".into(),
                label: "Focused Edges".into(),
                description: "Only flip edges participating in violations".into(),
                param_type: ParamType::Bool,
                default: serde_json::json!(false),
            },
            ConfigParam {
                name: "target_k".into(),
                label: "Target K".into(),
                description: "Clique size to minimize in graph (red)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
            },
            ConfigParam {
                name: "target_ell".into(),
                label: "Target Ell".into(),
                description: "Clique size to minimize in complement (blue)".into(),
                param_type: ParamType::Int { min: 3, max: 10 },
                default: serde_json::json!(5),
            },
        ]
    }

    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult {
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
        let focused = job
            .config
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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

        // Build edge list
        let all_edges: Vec<(u32, u32)> = {
            let mut v = Vec::with_capacity((n * (n - 1) / 2) as usize);
            for i in 0..n {
                for j in (i + 1)..n {
                    v.push((i, j));
                }
            }
            v
        };

        // Seed graph
        let seed = job
            .init_graph
            .clone()
            .unwrap_or_else(|| crate::init::random_graph(n, &mut rng));

        let seed_entry = BeamEntry::from_graph(seed, k, ell);

        let mut iters_used: u64 = 0;
        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64)> = None;

        // Fingerprint dedup
        let mut seen: HashSet<u64> = HashSet::new();
        let mut known_cids = job.known_cids.clone();

        seen.insert(fast_fingerprint(&seed_entry.adj_nbrs.masks));

        // Report initial state
        report_progress(
            observer,
            &seed_entry,
            n,
            iters_used,
            max_iters,
            discovery_count,
        );

        // Check if seed is already valid
        if seed_entry.violations == 0 {
            let (canonical, _) = canonical_form(&seed_entry.graph);
            let cid = minegraph_graph::compute_cid(&canonical);
            if known_cids.insert(cid) {
                observer.on_discovery(&RawDiscovery {
                    graph: seed_entry.graph.clone(),
                    iteration: 0,
                });
                discovery_count += 1;
                best_valid = Some(seed_entry.graph.clone());
            }
        }

        if seed_entry.violations > 0 {
            best_invalid = Some((seed_entry.graph.clone(), seed_entry.violations));
        }

        let mut beam: Vec<BeamEntry> = vec![seed_entry];

        for depth in 0..max_depth {
            if iters_used >= max_iters || beam.is_empty() || observer.is_cancelled() {
                break;
            }

            let depth_start = Instant::now();
            let remaining = max_iters.saturating_sub(iters_used);

            // Candidate scores: (parent_idx, u, v, new_violations, new_kc, new_ei)
            let mut candidates: Vec<(usize, u32, u32, u64, u64, u64)> = Vec::new();
            let mut dedup_hits: u64 = 0;
            let mut scored_count: u64 = 0;
            let beam_len = beam.len();

            // Focused mode: compute guilty edges once per depth
            let depth_edges: Vec<(u32, u32)> = if focused {
                let best = beam.iter().min_by_key(|e| e.violations).unwrap();
                if best.violations > 0 {
                    let ge = guilty_edges(&best.adj_nbrs, &best.comp_nbrs, k, ell, n);
                    if ge.is_empty() { all_edges.clone() } else { ge }
                } else {
                    all_edges.clone()
                }
            } else {
                all_edges.clone()
            };
            let depth_edge_count = depth_edges.len();

            for (parent_idx, parent) in beam.iter_mut().enumerate() {
                if iters_used >= max_iters || observer.is_cancelled() {
                    break;
                }

                // Budget per parent
                let edges_to_try = {
                    let per_parent = remaining.saturating_sub(scored_count)
                        / (beam_len - parent_idx).max(1) as u64;
                    if (depth_edge_count as u64) > per_parent && per_parent > 0 {
                        per_parent as usize
                    } else {
                        depth_edge_count
                    }
                };

                let edges_iter: Vec<(u32, u32)> = if edges_to_try < depth_edge_count {
                    let mut indices: Vec<usize> = (0..depth_edge_count).collect();
                    let (selected, _) = indices.partial_shuffle(&mut rng, edges_to_try);
                    selected.iter().map(|&i| depth_edges[i]).collect()
                } else {
                    depth_edges.clone()
                };

                for &(u, v) in &edges_iter {
                    if iters_used >= max_iters || observer.is_cancelled() {
                        break;
                    }

                    // Flip-score-unflip
                    parent.flip(u, v);
                    let fp = fast_fingerprint(&parent.adj_nbrs.masks);
                    if !seen.insert(fp) {
                        parent.flip(u, v);
                        dedup_hits += 1;
                        continue;
                    }
                    parent.flip(u, v);

                    // Incremental delta
                    let (delta_kc, delta_ei) =
                        violation_delta(&parent.adj_nbrs, &parent.comp_nbrs, k, ell, u, v);
                    let new_kc = (parent.kc as i64 + delta_kc).max(0) as u64;
                    let new_ei = (parent.ei as i64 + delta_ei).max(0) as u64;
                    let new_violations = new_kc + new_ei;

                    iters_used += 1;
                    scored_count += 1;

                    candidates.push((parent_idx, u, v, new_violations, new_kc, new_ei));

                    // Valid graph found
                    if new_violations == 0 {
                        let mut valid_graph = parent.graph.clone();
                        valid_graph.set_edge(u, v, !valid_graph.edge(u, v));

                        // Full recount to verify
                        let adj = NeighborSet::from_adj(&valid_graph);
                        let comp = valid_graph.complement();
                        let comp_n = NeighborSet::from_adj(&comp);
                        let actual_kc = count_cliques(&adj, k, n);
                        let actual_ei = count_cliques(&comp_n, ell, n);

                        if actual_kc + actual_ei == 0 {
                            // Canonical CID for dedup (nauty call — only on valid graphs)
                            let (canonical, _) = canonical_form(&valid_graph);
                            let cid = minegraph_graph::compute_cid(&canonical);
                            if known_cids.insert(cid) {
                                observer.on_discovery(&RawDiscovery {
                                    graph: valid_graph.clone(),
                                    iteration: iters_used,
                                });
                                discovery_count += 1;
                                best_valid = Some(valid_graph);
                            }
                        }
                    }

                    // Track best invalid
                    if new_violations > 0 {
                        let dominated = best_invalid
                            .as_ref()
                            .is_none_or(|(_, bv)| new_violations < *bv);
                        if dominated {
                            let mut g = parent.graph.clone();
                            g.set_edge(u, v, !g.edge(u, v));
                            best_invalid = Some((g, new_violations));
                        }
                    }

                    // Periodic progress (use parent since we're in iter_mut)
                    if iters_used.is_multiple_of(100) {
                        observer.on_progress(&ProgressInfo {
                            graph: parent.graph.clone(),
                            n,
                            strategy: "tree2".to_string(),
                            iteration: iters_used,
                            max_iters,
                            valid: best_valid.is_some(),
                            violation_score: parent.violations as u32,
                            discoveries_so_far: discovery_count,
                        });
                    }
                }
            }

            if candidates.is_empty() {
                debug!(depth, "tree2: no candidates, stopping");
                break;
            }

            // Select top beam_width by violation score
            candidates.sort_by_key(|&(_, _, _, v, _, _)| v);
            candidates.truncate(beam_width);

            // Materialize new beam
            let do_full_recount = depth % 5 == 0;
            let mut new_beam: Vec<BeamEntry> = Vec::with_capacity(candidates.len());
            for &(parent_idx, cu, cv, _new_v, new_kc, new_ei) in &candidates {
                let parent = &beam[parent_idx];
                let mut graph = parent.graph.clone();
                let mut comp = parent.comp.clone();
                let cur = graph.edge(cu, cv);
                graph.set_edge(cu, cv, !cur);
                comp.set_edge(cu, cv, cur);

                let adj_nbrs = NeighborSet::from_adj(&graph);
                let comp_nbrs = NeighborSet::from_adj(&comp);

                let (kc, ei) = if do_full_recount {
                    (
                        count_cliques(&adj_nbrs, k, n),
                        count_cliques(&comp_nbrs, ell, n),
                    )
                } else {
                    (new_kc, new_ei)
                };

                new_beam.push(BeamEntry {
                    graph,
                    comp,
                    adj_nbrs,
                    comp_nbrs,
                    violations: kc + ei,
                    kc,
                    ei,
                });
            }

            let depth_elapsed = depth_start.elapsed();
            debug!(
                depth,
                beam_size = new_beam.len(),
                candidates = scored_count,
                dedup_hits,
                best = new_beam.first().map(|e| e.violations).unwrap_or(0),
                worst = new_beam.last().map(|e| e.violations).unwrap_or(0),
                discoveries = discovery_count,
                seen = seen.len(),
                edges = depth_edge_count,
                ms = depth_elapsed.as_millis() as u64,
                "tree2: depth complete"
            );

            beam = new_beam;
        }

        // Final progress
        if let Some(entry) = beam.first() {
            report_progress(observer, entry, n, iters_used, max_iters, discovery_count);
        }

        let has_valid = best_valid.is_some();
        let best = best_valid.or(best_invalid.map(|(g, _)| g));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: iters_used,
            discoveries: Vec::new(),
            carry_state: None,
        }
    }
}

fn report_progress(
    observer: &dyn SearchObserver,
    entry: &BeamEntry,
    n: u32,
    iteration: u64,
    max_iters: u64,
    discoveries: u64,
) {
    observer.on_progress(&ProgressInfo {
        graph: entry.graph.clone(),
        n,
        strategy: "tree2".to_string(),
        iteration,
        max_iters,
        valid: entry.violations == 0,
        violation_score: entry.violations as u32,
        discoveries_so_far: discoveries,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::paley_graph;
    use minegraph_worker_api::NoOpObserver;

    fn make_job(n: u32, k: u32, ell: u32, max_iters: u64) -> SearchJob {
        SearchJob {
            n,
            max_iters,
            seed: 42,
            init_graph: None,
            config: serde_json::json!({"target_k": k, "target_ell": ell}),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

    #[test]
    fn tree2_finds_valid_r33_n5() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn tree2_respects_budget() {
        let max = 500u64;
        let mut job = make_job(10, 4, 4, max);
        job.init_graph = Some(paley_graph(10));
        job.config =
            serde_json::json!({"beam_width": 10, "max_depth": 5, "target_k": 4, "target_ell": 4});
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(
            result.iterations_used <= max,
            "used {} but budget was {}",
            result.iterations_used,
            max
        );
    }

    #[test]
    fn tree2_finds_valid_r44_n17() {
        let mut job = make_job(17, 4, 4, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config =
            serde_json::json!({"beam_width": 50, "max_depth": 15, "target_k": 4, "target_ell": 4});
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(4,4) on 17 vertices");
    }

    #[test]
    fn tree2_focused_r33_n5() {
        let mut job = make_job(5, 3, 3, 50_000);
        job.init_graph = Some(paley_graph(5));
        job.config = serde_json::json!({"focused": true, "target_k": 3, "target_ell": 3});
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "focused tree2 should find valid R(3,3) on 5 vertices"
        );
    }

    #[test]
    fn tree2_focused_r44_n17() {
        let mut job = make_job(17, 4, 4, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config = serde_json::json!({"beam_width": 50, "max_depth": 15, "focused": true, "target_k": 4, "target_ell": 4});
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(
            result.valid,
            "focused tree2 should find valid R(4,4) on 17 vertices"
        );
    }
}
