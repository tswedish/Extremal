//! Evolutionary simulated annealing strategy for Ramsey graph search.
//!
//! Maintains a small population (default 4) of candidate graphs. Each
//! individual runs SA independently: flip a random edge, accept if
//! violations decrease (or with SA probability if they increase).
//! Periodically, the best individual's neighborhood structure is
//! cross-pollinated to weaker individuals.
//!
//! # Incremental violation counting
//!
//! Instead of recomputing all k-cliques from scratch on every edge flip,
//! we only count cliques that pass through the flipped edge endpoints.
//! For R(5,5) n=25, flipping edge (u,v) affects at most C(23,3) = 1,771
//! potential 5-cliques per color, vs C(25,5) = 53,130 for a full recount.
//!
//! # Cross-round persistence
//!
//! The population is serialized into `SearchResult::carry_state` and
//! restored from `SearchJob::carry_state` on subsequent rounds. This
//! lets the population evolve across server sync boundaries.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_verifier::clique::count_cliques;
use ramseynet_worker_api::{
    ConfigParam, ParamType, ProgressInfo, RawDiscovery, SearchJob, SearchObserver, SearchResult,
    SearchStrategy,
};

// ── Incremental violation counting ──────────────────────────────────

/// Count k-cliques that contain BOTH vertices u and v.
/// This is the delta-relevant set when edge (u,v) is flipped.
/// Returns 0 if edge (u,v) is not present (no clique can contain a non-edge).
fn count_cliques_through_edge(adj: &AdjacencyMatrix, k: u32, u: u32, v: u32) -> u64 {
    if k < 2 {
        return 0;
    }
    // A clique through (u,v) requires the edge (u,v) to exist
    if !adj.edge(u, v) {
        return 0;
    }
    if k == 2 {
        return 1;
    }
    let n = adj.n();
    // Find common neighbors of u and v (candidates for remaining k-2 vertices)
    let common: Vec<u32> = (0..n)
        .filter(|&w| w != u && w != v && adj.edge(u, w) && adj.edge(v, w))
        .collect();

    if (common.len() as u32) < k - 2 {
        return 0;
    }

    // Count (k-2)-cliques among the common neighbors
    let mut count = 0u64;
    let mut current = Vec::with_capacity((k - 2) as usize);
    count_cliques_in_subset(adj, &common, &mut current, 0, k - 2, &mut count);
    count
}

/// Count cliques of size `target` using only vertices from `candidates`.
fn count_cliques_in_subset(
    adj: &AdjacencyMatrix,
    candidates: &[u32],
    current: &mut Vec<u32>,
    start: usize,
    target: u32,
    count: &mut u64,
) {
    if current.len() as u32 == target {
        *count += 1;
        return;
    }
    let remaining = target - current.len() as u32;
    if candidates.len() - start < remaining as usize {
        return;
    }
    for i in start..candidates.len() {
        let v = candidates[i];
        if current.iter().all(|&u| adj.edge(u, v)) {
            current.push(v);
            count_cliques_in_subset(adj, candidates, current, i + 1, target, count);
            current.pop();
        }
    }
}

/// Violation score: total k-cliques + ell-independent-sets.
/// An independent set of size ell in G is a clique of size ell in complement(G).
fn full_violation_score(adj: &AdjacencyMatrix, k: u32, ell: u32) -> (u64, u64, u64) {
    let kc = count_cliques(adj, k);
    let comp = adj.complement();
    let ei = count_cliques(&comp, ell);
    (kc + ei, kc, ei)
}

/// Compute the change in violation score from flipping edge (u,v).
///
/// Returns (delta_kc, delta_ei): the change in k-clique count and
/// ell-independent-set count. These can be negative (improvement).
fn violation_delta(adj: &AdjacencyMatrix, k: u32, ell: u32, u: u32, v: u32) -> (i64, i64) {
    // Build the "after" state for both G and complement
    let mut g_after = adj.clone();
    let cur = g_after.edge(u, v);
    g_after.set_edge(u, v, !cur);

    // k-cliques through (u,v) before and after
    let kc_before = count_cliques_through_edge(adj, k, u, v) as i64;
    let kc_after = count_cliques_through_edge(&g_after, k, u, v) as i64;

    // ell-cliques in complement through (u,v) before and after
    let comp_before = adj.complement();
    let comp_after = g_after.complement();
    let ei_before = count_cliques_through_edge(&comp_before, ell, u, v) as i64;
    let ei_after = count_cliques_through_edge(&comp_after, ell, u, v) as i64;

    (kc_after - kc_before, ei_after - ei_before)
}

// ── Population state (persisted across rounds) ──────────────────────

/// State for one individual in the population.
struct Individual {
    graph: AdjacencyMatrix,
    violations: u64,
    kc: u64,
    ei: u64,
}

/// Population state carried across rounds.
struct PopulationState {
    individuals: Vec<(AdjacencyMatrix, u64, u64, u64)>, // (graph, violations, kc, ei)
}

// ── EvoSearch strategy ──────────────────────────────────────────────

pub struct EvoSearch;

impl SearchStrategy for EvoSearch {
    fn id(&self) -> &str {
        "evo"
    }

    fn name(&self) -> &str {
        "Evolutionary SA"
    }

    fn config_schema(&self) -> Vec<ConfigParam> {
        vec![
            ConfigParam {
                name: "pop_size".into(),
                label: "Population Size".into(),
                description: "Number of individuals in the population".into(),
                param_type: ParamType::Int { min: 1, max: 32 },
                default: serde_json::json!(4),
            },
            ConfigParam {
                name: "temp_start".into(),
                label: "Start Temperature".into(),
                description: "Initial SA temperature (higher = more exploration)".into(),
                param_type: ParamType::Float {
                    min: 0.01,
                    max: 100.0,
                },
                default: serde_json::json!(2.0),
            },
            ConfigParam {
                name: "temp_end".into(),
                label: "End Temperature".into(),
                description: "Final SA temperature".into(),
                param_type: ParamType::Float {
                    min: 0.001,
                    max: 10.0,
                },
                default: serde_json::json!(0.01),
            },
            ConfigParam {
                name: "crossover_interval".into(),
                label: "Crossover Interval".into(),
                description: "Iterations between crossover events (0 = disabled)".into(),
                param_type: ParamType::Int {
                    min: 0,
                    max: 1_000_000,
                },
                default: serde_json::json!(5000),
            },
            ConfigParam {
                name: "restart_stale".into(),
                label: "Restart Stale After".into(),
                description:
                    "Restart an individual if no improvement for this many iters (0 = disabled)"
                        .into(),
                param_type: ParamType::Int {
                    min: 0,
                    max: 1_000_000,
                },
                default: serde_json::json!(50000),
            },
        ]
    }

    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult {
        let pop_size = job
            .config
            .get("pop_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize;
        let temp_start = job
            .config
            .get("temp_start")
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0);
        let temp_end = job
            .config
            .get("temp_end")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.01);
        let crossover_interval = job
            .config
            .get("crossover_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000);
        let restart_stale = job
            .config
            .get("restart_stale")
            .and_then(|v| v.as_u64())
            .unwrap_or(50000);

        let n = job.n;
        let k = job.k;
        let ell = job.ell;
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
        let num_edges = all_edges.len();

        // ── Initialize population ────────────────────────────────
        // Try to restore from carry_state, otherwise build fresh
        let mut pop: Vec<Individual> = Vec::with_capacity(pop_size);

        if let Some(state) = job
            .carry_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<PopulationState>())
        {
            // Restore persisted population
            for (graph, violations, kc, ei) in &state.individuals {
                if pop.len() >= pop_size {
                    break;
                }
                pop.push(Individual {
                    graph: graph.clone(),
                    violations: *violations,
                    kc: *kc,
                    ei: *ei,
                });
            }
        }

        // Fill remaining slots with the seed graph (with perturbation for diversity)
        while pop.len() < pop_size {
            let graph = if pop.is_empty() {
                // First individual: use seed graph directly
                job.init_graph
                    .clone()
                    .unwrap_or_else(|| random_graph(n, &mut rng))
            } else {
                // Additional individuals: perturb the seed
                let mut g = job
                    .init_graph
                    .clone()
                    .unwrap_or_else(|| random_graph(n, &mut rng));
                let flips = (num_edges as f64).sqrt().ceil() as u32;
                for _ in 0..flips {
                    let &(i, j) = &all_edges[rng.gen_range(0..num_edges)];
                    let cur = g.edge(i, j);
                    g.set_edge(i, j, !cur);
                }
                g
            };
            let (violations, kc, ei) = full_violation_score(&graph, k, ell);
            pop.push(Individual {
                graph,
                violations,
                kc,
                ei,
            });
        }

        let mut best_valid: Option<AdjacencyMatrix> = None;
        let mut discovery_count: u64 = 0;
        let mut iters_used: u64 = 0;

        // Per-individual stale counters (iters since last improvement)
        let mut stale_counters: Vec<u64> = vec![0; pop_size];

        // Temperature schedule: exponential cooling
        let temp_ratio = (temp_end / temp_start).ln();

        // Report initial state
        let best_idx = best_individual(&pop);
        report_progress(
            observer,
            &pop[best_idx],
            n,
            k,
            ell,
            iters_used,
            max_iters,
            discovery_count,
        );

        // Check if any initial individual is already valid
        for ind in &pop {
            if ind.violations == 0 {
                observer.on_discovery(&RawDiscovery {
                    graph: ind.graph.clone(),
                    iteration: iters_used,
                });
                discovery_count += 1;
                best_valid = Some(ind.graph.clone());
            }
        }

        // ── Main SA loop ─────────────────────────────────────────
        // Iterations are distributed round-robin across the population.
        while iters_used < max_iters && !observer.is_cancelled() {
            let ind_idx = (iters_used as usize) % pop_size;
            let progress = iters_used as f64 / max_iters as f64;
            let temperature = temp_start * (temp_ratio * progress).exp();

            // Pick a random edge to flip
            let edge_idx = rng.gen_range(0..num_edges);
            let (u, v) = all_edges[edge_idx];

            let ind = &mut pop[ind_idx];

            // Incremental delta computation
            let (delta_kc, delta_ei) = violation_delta(&ind.graph, k, ell, u, v);
            let old_violations = ind.violations as i64;
            let new_violations = old_violations + delta_kc + delta_ei;

            let accept = if new_violations <= old_violations {
                true // always accept improvements or equal
            } else if temperature > 1e-10 {
                let delta = (new_violations - old_violations) as f64;
                let prob = (-delta / temperature).exp();
                rng.gen::<f64>() < prob
            } else {
                false
            };

            if accept {
                // Apply the flip
                let cur = ind.graph.edge(u, v);
                ind.graph.set_edge(u, v, !cur);
                ind.kc = (ind.kc as i64 + delta_kc).max(0) as u64;
                ind.ei = (ind.ei as i64 + delta_ei).max(0) as u64;
                ind.violations = (new_violations.max(0)) as u64;

                if new_violations < old_violations {
                    stale_counters[ind_idx] = 0;
                } else {
                    stale_counters[ind_idx] += 1;
                }

                // Check for valid graph
                if ind.violations == 0 {
                    // Double-check with full recount (incremental can drift)
                    let (actual, actual_kc, actual_ei) = full_violation_score(&ind.graph, k, ell);
                    ind.violations = actual;
                    ind.kc = actual_kc;
                    ind.ei = actual_ei;

                    if actual == 0 {
                        observer.on_discovery(&RawDiscovery {
                            graph: ind.graph.clone(),
                            iteration: iters_used,
                        });
                        discovery_count += 1;
                        best_valid = Some(ind.graph.clone());
                    }
                }
            } else {
                stale_counters[ind_idx] += 1;
            }

            iters_used += 1;

            // Periodic progress report (every 500 iters)
            if iters_used.is_multiple_of(500) {
                let bi = best_individual(&pop);
                report_progress(
                    observer,
                    &pop[bi],
                    n,
                    k,
                    ell,
                    iters_used,
                    max_iters,
                    discovery_count,
                );
            }

            // Crossover: best individual shares structure with worst
            if crossover_interval > 0
                && iters_used.is_multiple_of(crossover_interval)
                && pop_size > 1
            {
                let bi = best_individual(&pop);
                let wi = worst_individual(&pop);
                if bi != wi {
                    // Row-swap crossover: pick a random vertex and copy its
                    // entire neighborhood from the best to the worst.
                    let vertex = rng.gen_range(0..n);
                    for other in 0..n {
                        if other != vertex {
                            let edge_val = pop[bi].graph.edge(vertex, other);
                            pop[wi].graph.set_edge(vertex, other, edge_val);
                        }
                    }
                    // Full recount after crossover
                    let (v, kc, ei) = full_violation_score(&pop[wi].graph, k, ell);
                    pop[wi].violations = v;
                    pop[wi].kc = kc;
                    pop[wi].ei = ei;
                    stale_counters[wi] = 0;
                }
            }

            // Restart stale individuals
            if restart_stale > 0 && iters_used.is_multiple_of(1000) {
                for i in 0..pop_size {
                    if stale_counters[i] >= restart_stale {
                        // Re-initialize from seed graph with heavy perturbation
                        let mut g = job
                            .init_graph
                            .clone()
                            .unwrap_or_else(|| random_graph(n, &mut rng));
                        let flips = num_edges / 4; // 25% perturbation
                        for _ in 0..flips {
                            let &(ei, ej) = &all_edges[rng.gen_range(0..num_edges)];
                            let cur = g.edge(ei, ej);
                            g.set_edge(ei, ej, !cur);
                        }
                        let (v, kc, ei) = full_violation_score(&g, k, ell);
                        pop[i] = Individual {
                            graph: g,
                            violations: v,
                            kc,
                            ei,
                        };
                        stale_counters[i] = 0;
                    }
                }
            }

            // Periodic full recount to prevent drift (every 10K iters per individual)
            if iters_used.is_multiple_of(10_000 * pop_size as u64) {
                for ind in &mut pop {
                    let (v, kc, ei) = full_violation_score(&ind.graph, k, ell);
                    ind.violations = v;
                    ind.kc = kc;
                    ind.ei = ei;
                }
            }
        }

        // Final progress report
        let bi = best_individual(&pop);
        report_progress(
            observer,
            &pop[bi],
            n,
            k,
            ell,
            iters_used,
            max_iters,
            discovery_count,
        );

        // Build carry_state for next round
        let carry = PopulationState {
            individuals: pop
                .iter()
                .map(|ind| (ind.graph.clone(), ind.violations, ind.kc, ind.ei))
                .collect(),
        };

        let has_valid = best_valid.is_some();
        let best_graph = best_valid.or_else(|| {
            let bi = best_individual(&pop);
            Some(pop[bi].graph.clone())
        });

        SearchResult {
            valid: has_valid,
            best_graph,
            iterations_used: iters_used,
            discoveries: Vec::new(), // all streamed via on_discovery
            carry_state: Some(Box::new(carry)),
        }
    }
}

fn best_individual(pop: &[Individual]) -> usize {
    pop.iter()
        .enumerate()
        .min_by_key(|(_, ind)| ind.violations)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn worst_individual(pop: &[Individual]) -> usize {
    pop.iter()
        .enumerate()
        .max_by_key(|(_, ind)| ind.violations)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn report_progress(
    observer: &dyn SearchObserver,
    ind: &Individual,
    n: u32,
    k: u32,
    ell: u32,
    iteration: u64,
    max_iters: u64,
    discoveries: u64,
) {
    observer.on_progress(&ProgressInfo {
        graph: ind.graph.clone(),
        n,
        k,
        ell,
        strategy: "evo".to_string(),
        iteration,
        max_iters,
        valid: ind.violations == 0,
        violation_score: ind.violations as u32,
        discoveries_so_far: discoveries,
        k_cliques: Some(ind.kc),
        ell_indsets: Some(ind.ei),
    });
}

fn random_graph(n: u32, rng: &mut SmallRng) -> AdjacencyMatrix {
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
    use std::collections::HashSet;

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

    // ── Incremental scoring tests ─────────────────────────────

    #[test]
    fn incremental_delta_matches_full_recount() {
        // Test that violation_delta produces correct results by comparing
        // against full recount before and after flipping.
        let mut rng = SmallRng::seed_from_u64(123);
        let n = 10;
        let k = 3;
        let ell = 3;

        let mut g = AdjacencyMatrix::new(n);
        for i in 0..n {
            for j in (i + 1)..n {
                if rng.gen_bool(0.5) {
                    g.set_edge(i, j, true);
                }
            }
        }

        // Test several random edge flips
        for _ in 0..50 {
            let u = rng.gen_range(0..n);
            let v = rng.gen_range(0..n);
            if u == v {
                continue;
            }
            let (u, v) = if u < v { (u, v) } else { (v, u) };

            let (_before_total, before_kc, before_ei) = full_violation_score(&g, k, ell);
            let (delta_kc, delta_ei) = violation_delta(&g, k, ell, u, v);

            // Apply flip
            let cur = g.edge(u, v);
            g.set_edge(u, v, !cur);

            let (_after_total, after_kc, after_ei) = full_violation_score(&g, k, ell);

            assert_eq!(
                after_kc as i64 - before_kc as i64,
                delta_kc,
                "kc delta mismatch: before={before_kc} after={after_kc} delta={delta_kc}"
            );
            assert_eq!(
                after_ei as i64 - before_ei as i64,
                delta_ei,
                "ei delta mismatch: before={before_ei} after={after_ei} delta={delta_ei}"
            );
        }
    }

    #[test]
    fn count_cliques_through_edge_k2() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        // 2-cliques through (0,1): just the edge itself
        assert_eq!(count_cliques_through_edge(&g, 2, 0, 1), 1);
        // No edge (0,2)
        assert_eq!(count_cliques_through_edge(&g, 2, 0, 2), 0);
    }

    #[test]
    fn count_cliques_through_edge_triangle() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(0, 2, true);
        // 3-cliques through edge (0,1): {0,1,2}
        assert_eq!(count_cliques_through_edge(&g, 3, 0, 1), 1);
    }

    // ── Strategy integration tests ────────────────────────────

    #[test]
    fn evo_finds_valid_r33_n5() {
        let mut job = make_job(3, 3, 5, 50_000);
        job.init_graph = Some(paley_graph(5));
        job.config = serde_json::json!({"pop_size": 2});
        let result = EvoSearch.search(&job, &NoOpObserver);
        assert!(result.valid, "should find valid R(3,3) on 5 vertices");
    }

    #[test]
    fn evo_carry_state_roundtrip() {
        let mut job = make_job(3, 3, 5, 1_000);
        job.init_graph = Some(paley_graph(5));
        job.config = serde_json::json!({"pop_size": 2});

        // Round 1
        let result1 = EvoSearch.search(&job, &NoOpObserver);
        assert!(result1.carry_state.is_some());

        // Round 2: restore state
        job.carry_state = result1.carry_state;
        job.seed = 99; // different seed
        let result2 = EvoSearch.search(&job, &NoOpObserver);
        assert!(result2.carry_state.is_some());
    }

    #[test]
    fn evo_respects_budget() {
        let max = 500u64;
        let mut job = make_job(4, 4, 10, max);
        job.init_graph = Some(paley_graph(10));
        job.config = serde_json::json!({"pop_size": 2});
        let result = EvoSearch.search(&job, &NoOpObserver);
        assert!(
            result.iterations_used <= max,
            "used {} but budget was {}",
            result.iterations_used,
            max
        );
    }
}
