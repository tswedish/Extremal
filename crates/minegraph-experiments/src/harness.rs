//! Benchmark harness for comparing search strategies.
//!
//! Measures search *quality* (discoveries, violations) not just speed.
//! Runs each strategy on a standardized problem set with controlled seeds.

use std::collections::HashSet;
use std::time::Instant;

use minegraph_strategies::init::paley_graph;
use minegraph_worker_api::{CollectingObserver, SearchJob, SearchStrategy};
use serde::{Deserialize, Serialize};

/// Result of running one strategy on one problem instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchResult {
    pub strategy_id: String,
    pub n: u32,
    pub k: u32,
    pub ell: u32,
    pub config: serde_json::Value,
    pub seeds_tested: u32,
    pub total_discoveries: u64,
    pub mean_iters_to_first: Option<f64>,
    pub mean_discoveries_per_round: f64,
    pub mean_round_ms: f64,
    pub best_violation_score: u64,
}

/// A standardized problem for benchmarking.
#[derive(Clone, Debug)]
pub struct Problem {
    pub name: &'static str,
    pub n: u32,
    pub k: u32,
    pub ell: u32,
}

/// The standardized problem set.
pub fn standard_problems() -> Vec<Problem> {
    vec![
        Problem {
            name: "R(3,3)/n=5",
            n: 5,
            k: 3,
            ell: 3,
        },
        Problem {
            name: "R(4,4)/n=17",
            n: 17,
            k: 4,
            ell: 4,
        },
        Problem {
            name: "R(5,5)/n=25",
            n: 25,
            k: 5,
            ell: 5,
        },
        Problem {
            name: "R(5,5)/n=43",
            n: 43,
            k: 5,
            ell: 5,
        },
    ]
}

/// Run a strategy on a single problem, averaging over multiple seeds.
pub fn bench_strategy(
    strategy: &dyn SearchStrategy,
    problem: &Problem,
    budget: u64,
    num_seeds: u32,
) -> BenchResult {
    let config = build_config(problem.k, problem.ell);

    let mut total_discoveries: u64 = 0;
    let mut total_ms: f64 = 0.0;
    let mut iters_to_first: Vec<f64> = Vec::new();
    let mut best_violation: u64 = u64::MAX;

    for seed_idx in 0..num_seeds {
        let seed_graph = paley_graph(problem.n);

        let job = SearchJob {
            n: problem.n,
            max_iters: budget,
            seed: seed_idx as u64,
            init_graph: Some(seed_graph),
            config: config.clone(),
            known_cids: HashSet::new(),
            max_known_cids: 10_000,
            carry_state: None,
        };

        let observer = CollectingObserver::new();
        let start = Instant::now();
        let result = strategy.search(&job, &observer);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        let discoveries = observer.drain();
        let round_discoveries = discoveries.len() as u64;

        total_discoveries += round_discoveries;
        total_ms += elapsed_ms;

        // Track first discovery iteration
        if let Some(first) = discoveries.first() {
            iters_to_first.push(first.iteration as f64);
        }

        // Track best violation score (0 = valid graph found)
        if result.valid {
            best_violation = 0;
        } else if let Some(iters) = result.iterations_used.checked_sub(0) {
            // Use iterations_used as a proxy; real violation tracking would
            // need the strategy to report it
            best_violation = best_violation.min(iters);
        }
    }

    let mean_round_ms = total_ms / num_seeds as f64;
    let mean_discoveries_per_round = total_discoveries as f64 / num_seeds as f64;
    let mean_iters_to_first = if iters_to_first.is_empty() {
        None
    } else {
        Some(iters_to_first.iter().sum::<f64>() / iters_to_first.len() as f64)
    };

    BenchResult {
        strategy_id: strategy.id().to_string(),
        n: problem.n,
        k: problem.k,
        ell: problem.ell,
        config,
        seeds_tested: num_seeds,
        total_discoveries,
        mean_iters_to_first,
        mean_discoveries_per_round,
        mean_round_ms,
        best_violation_score: if best_violation == u64::MAX {
            0
        } else {
            best_violation
        },
    }
}

/// Run all strategies against a specific problem.
pub fn compare_strategies(
    strategies: &[Box<dyn SearchStrategy>],
    problem: &Problem,
    budget: u64,
    num_seeds: u32,
) -> Vec<BenchResult> {
    strategies
        .iter()
        .map(|s| bench_strategy(s.as_ref(), problem, budget, num_seeds))
        .collect()
}

/// Print a comparison table to stdout.
pub fn print_results(problem: &Problem, results: &[BenchResult]) {
    println!(
        "\n{} (n={}, k={}, ell={})",
        problem.name, problem.n, problem.k, problem.ell
    );
    println!(
        "{:<15} {:>8} {:>12} {:>14} {:>12}",
        "Strategy", "Seeds", "Discoveries", "Mean Disc/Rnd", "Mean ms/Rnd"
    );
    println!("{}", "-".repeat(65));
    for r in results {
        println!(
            "{:<15} {:>8} {:>12} {:>14.2} {:>12.1}",
            r.strategy_id,
            r.seeds_tested,
            r.total_discoveries,
            r.mean_discoveries_per_round,
            r.mean_round_ms,
        );
    }
}

fn build_config(k: u32, ell: u32) -> serde_json::Value {
    serde_json::json!({
        "target_k": k,
        "target_ell": ell,
    })
}
