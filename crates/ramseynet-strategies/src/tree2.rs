//! Incremental beam search over single-edge flips (tree2).
//!
//! Same beam search structure as tree1, but with three key optimizations:
//!
//! 1. **Flip-score-unflip**: Instead of cloning the parent for each edge,
//!    we flip in-place, compute the incremental delta, then unflip.
//!    Zero allocations per candidate in the hot loop.
//!
//! 2. **Incremental violation delta**: Uses `violation_delta` from the
//!    shared incremental module. For k=5 n=25: ~1,771 subset checks
//!    instead of ~53,130 per candidate.
//!
//! 3. **Cheap 64-bit fingerprint**: Uses XOR-fold for beam dedup instead
//!    of SHA-256. Full CID only computed when reporting a valid discovery.
//!
//! Each beam entry carries its complement for zero-alloc delta computation.

use std::collections::HashSet;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use ramseynet_graph::{compute_cid, AdjacencyMatrix};
use ramseynet_verifier::clique::count_cliques;
use ramseynet_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};
use tracing::debug;

use crate::incremental::{fast_fingerprint, violation_delta_bitwise, NeighborSet};

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
        let kc = count_cliques(&graph, k);
        let ei = count_cliques(&comp, ell);
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

        let n = job.n;
        let k = job.k;
        let ell = job.ell;
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
        let num_edges = all_edges.len();

        // Seed graph
        let seed = job.init_graph.clone().unwrap_or_else(|| {
            let mut g = AdjacencyMatrix::new(n);
            for i in 0..n {
                for j in (i + 1)..n {
                    if rng.gen_bool(0.5) {
                        g.set_edge(i, j, true);
                    }
                }
            }
            g
        });

        let seed_entry = BeamEntry::from_graph(seed, k, ell);

        let mut iters_used: u64 = 0;
        let mut discovery_count: u64 = 0;
        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut best_invalid: Option<(AdjacencyMatrix, u64, u64, u64)> = None; // (graph, violations, kc, ei)

        // Fingerprint-based dedup (cheap 64-bit hash)
        let mut seen: HashSet<u64> = HashSet::new();
        // Also track full CIDs for known server graphs
        let mut known_cids: HashSet<ramseynet_types::GraphCid> = job.known_cids.clone();

        // Seed the seen set
        seen.insert(fast_fingerprint(&seed_entry.graph));

        // Report initial state
        report_progress(
            observer,
            &seed_entry,
            n,
            k,
            ell,
            iters_used,
            max_iters,
            discovery_count,
        );

        // Check if seed is already valid
        if seed_entry.violations == 0 {
            let cid = compute_cid(&seed_entry.graph);
            if known_cids.insert(cid) {
                observer.on_discovery(&RawDiscovery {
                    graph: seed_entry.graph.clone(),
                    iteration: 0,
                });
                discovery_count += 1;
                best_valid = Some(seed_entry.graph.clone());
            }
        }

        // Track best invalid for reporting
        if seed_entry.violations > 0 {
            best_invalid = Some((
                seed_entry.graph.clone(),
                seed_entry.violations,
                seed_entry.kc,
                seed_entry.ei,
            ));
        }

        // Current beam
        let mut beam: Vec<BeamEntry> = vec![seed_entry];

        for depth in 0..max_depth {
            if iters_used >= max_iters || beam.is_empty() || observer.is_cancelled() {
                break;
            }

            let depth_start = Instant::now();
            let remaining = max_iters.saturating_sub(iters_used);
            let full_budget = num_edges as u64 * beam.len() as u64;

            let edges_per_parent = if full_budget > remaining {
                let per = remaining / beam.len().max(1) as u64;
                (per as usize).max(1).min(num_edges)
            } else {
                num_edges
            };

            // Collect candidate scores: (parent_idx, edge_idx, new_violations, new_kc, new_ei)
            let mut candidates: Vec<(usize, usize, u64, u64, u64)> = Vec::new();
            let mut dedup_hits: u64 = 0;
            let mut scored_count: u64 = 0;

            for (parent_idx, parent) in beam.iter_mut().enumerate() {
                if iters_used >= max_iters || observer.is_cancelled() {
                    break;
                }

                // Select edges to try
                let edge_indices: Vec<usize> = if edges_per_parent < num_edges {
                    let mut indices: Vec<usize> = (0..num_edges).collect();
                    let (selected, _) = indices.partial_shuffle(&mut rng, edges_per_parent);
                    selected.to_vec()
                } else {
                    (0..num_edges).collect()
                };

                for &edge_idx in &edge_indices {
                    if iters_used >= max_iters || observer.is_cancelled() {
                        break;
                    }

                    let (u, v) = all_edges[edge_idx];

                    // Flip in-place
                    parent.flip(u, v);

                    // Cheap dedup
                    let fp = fast_fingerprint(&parent.graph);
                    if !seen.insert(fp) {
                        // Already seen — unflip and skip
                        parent.flip(u, v);
                        dedup_hits += 1;
                        continue;
                    }

                    // Unflip to compute delta from the original parent state
                    parent.flip(u, v);

                    // Incremental delta — bitwise, zero allocation
                    let (delta_kc, delta_ei) =
                        violation_delta_bitwise(&parent.adj_nbrs, &parent.comp_nbrs, k, ell, u, v);
                    let new_kc = (parent.kc as i64 + delta_kc).max(0) as u64;
                    let new_ei = (parent.ei as i64 + delta_ei).max(0) as u64;
                    let new_violations = new_kc + new_ei;

                    iters_used += 1;
                    scored_count += 1;

                    candidates.push((parent_idx, edge_idx, new_violations, new_kc, new_ei));

                    // Check for valid graph
                    if new_violations == 0 {
                        // Materialize and verify with full recount
                        let mut valid_graph = parent.graph.clone();
                        valid_graph.set_edge(u, v, !valid_graph.edge(u, v));

                        let actual_kc = count_cliques(&valid_graph, k);
                        let comp = valid_graph.complement();
                        let actual_ei = count_cliques(&comp, ell);

                        if actual_kc + actual_ei == 0 {
                            let cid = compute_cid(&valid_graph);
                            if known_cids.insert(cid) {
                                observer.on_discovery(&RawDiscovery {
                                    graph: valid_graph.clone(),
                                    iteration: iters_used,
                                });
                                discovery_count += 1;
                                report_progress_valid(
                                    observer,
                                    &valid_graph,
                                    n,
                                    k,
                                    ell,
                                    iters_used,
                                    max_iters,
                                    discovery_count,
                                );
                                best_valid = Some(valid_graph);
                            }
                        }
                    }

                    // Track best invalid
                    if let Some((_, best_v, _, _)) = &best_invalid {
                        if new_violations > 0 && new_violations < *best_v {
                            // Materialize just for tracking (cheap clone)
                            let mut g = parent.graph.clone();
                            g.set_edge(u, v, !g.edge(u, v));
                            best_invalid = Some((g, new_violations, new_kc, new_ei));
                        }
                    } else if new_violations > 0 {
                        let mut g = parent.graph.clone();
                        g.set_edge(u, v, !g.edge(u, v));
                        best_invalid = Some((g, new_violations, new_kc, new_ei));
                    }

                    // Periodic progress report
                    if iters_used.is_multiple_of(100) {
                        let (display_graph, display_v, display_kc, display_ei) =
                            if let Some(ref v) = best_valid {
                                (v, 0u64, 0u64, 0u64)
                            } else if let Some((ref g, v, kc, ei)) = best_invalid {
                                (g, v, kc, ei)
                            } else {
                                (&parent.graph, parent.violations, parent.kc, parent.ei)
                            };
                        observer.on_progress(&ProgressInfo {
                            graph: display_graph.clone(),
                            n,
                            k,
                            ell,
                            strategy: "tree2".to_string(),
                            iteration: iters_used,
                            max_iters,
                            valid: best_valid.is_some(),
                            violation_score: display_v as u32,
                            discoveries_so_far: discovery_count,
                            k_cliques: Some(display_kc),
                            ell_indsets: Some(display_ei),
                        });
                    }
                }
            }

            if candidates.is_empty() {
                debug!(depth, "tree2: no candidates generated, stopping");
                break;
            }

            // Select top beam_width candidates by violation score
            candidates.sort_by_key(|&(_, _, v, _, _)| v);
            candidates.truncate(beam_width);

            // Materialize the new beam: clone parent + apply flip + rebuild masks
            let mut new_beam: Vec<BeamEntry> = Vec::with_capacity(candidates.len());
            for &(parent_idx, edge_idx, _new_v, _new_kc, _new_ei) in &candidates {
                let (u, v) = all_edges[edge_idx];
                let parent = &beam[parent_idx];
                let mut graph = parent.graph.clone();
                let mut comp = parent.comp.clone();
                let cur = graph.edge(u, v);
                graph.set_edge(u, v, !cur);
                comp.set_edge(u, v, cur);

                // Build neighbor masks from the new graph state
                let adj_nbrs = NeighborSet::from_adj(&graph);
                let comp_nbrs = NeighborSet::from_adj(&comp);

                // Full recount for beam entries to prevent drift
                let kc = count_cliques(&graph, k);
                let ei = count_cliques(&comp, ell);
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
            let best_score = new_beam.first().map(|e| e.violations).unwrap_or(0);
            let worst_score = new_beam.last().map(|e| e.violations).unwrap_or(0);

            debug!(
                depth,
                beam_size = new_beam.len(),
                candidates = scored_count,
                dedup_hits,
                best_score,
                worst_score,
                discoveries = discovery_count,
                seen_set = seen.len(),
                elapsed_ms = depth_elapsed.as_millis() as u64,
                "tree2: depth complete"
            );

            beam = new_beam;
        }

        // Final progress report
        if let Some(ref v) = best_valid {
            report_progress_valid(
                observer,
                v,
                n,
                k,
                ell,
                iters_used,
                max_iters,
                discovery_count,
            );
        } else if let Some(entry) = beam.first() {
            report_progress(
                observer,
                entry,
                n,
                k,
                ell,
                iters_used,
                max_iters,
                discovery_count,
            );
        }

        let has_valid = best_valid.is_some();
        let best = best_valid.or(best_invalid.map(|(g, _, _, _)| g));

        SearchResult {
            valid: has_valid,
            best_graph: best,
            iterations_used: iters_used,
            discoveries: Vec::new(),
            carry_state: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn report_progress(
    observer: &dyn SearchObserver,
    entry: &BeamEntry,
    n: u32,
    k: u32,
    ell: u32,
    iteration: u64,
    max_iters: u64,
    discoveries: u64,
) {
    observer.on_progress(&ProgressInfo {
        graph: entry.graph.clone(),
        n,
        k,
        ell,
        strategy: "tree2".to_string(),
        iteration,
        max_iters,
        valid: entry.violations == 0,
        violation_score: entry.violations as u32,
        discoveries_so_far: discoveries,
        k_cliques: Some(entry.kc),
        ell_indsets: Some(entry.ei),
    });
}

#[allow(clippy::too_many_arguments)]
fn report_progress_valid(
    observer: &dyn SearchObserver,
    graph: &AdjacencyMatrix,
    n: u32,
    k: u32,
    ell: u32,
    iteration: u64,
    max_iters: u64,
    discoveries: u64,
) {
    observer.on_progress(&ProgressInfo {
        graph: graph.clone(),
        n,
        k,
        ell,
        strategy: "tree2".to_string(),
        iteration,
        max_iters,
        valid: true,
        violation_score: 0,
        discoveries_so_far: discoveries,
        k_cliques: Some(0),
        ell_indsets: Some(0),
    });
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
            init_graph: None,
            config: serde_json::json!({}),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        }
    }

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
    fn tree2_finds_valid_r33_n5() {
        let mut job = make_job(3, 3, 5, 50_000);
        job.init_graph = Some(paley_graph(5));
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn tree2_respects_budget() {
        let max = 500u64;
        let mut job = make_job(4, 4, 10, max);
        job.init_graph = Some(paley_graph(10));
        job.config = serde_json::json!({"beam_width": 10, "max_depth": 5});
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
        let mut job = make_job(4, 4, 17, 500_000);
        job.init_graph = Some(paley_graph(17));
        job.config = serde_json::json!({"beam_width": 50, "max_depth": 15});
        let result = Tree2Search.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(4,4) on 17 vertices");
    }
}
