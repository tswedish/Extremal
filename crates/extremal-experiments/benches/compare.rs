//! Criterion benchmarks for strategy comparison.
//!
//! Measures wall-clock time per search round. For quality metrics,
//! use the CLI harness instead (`cargo run -p extremal-experiments -- compare`).

use criterion::{Criterion, criterion_group, criterion_main};
use extremal_experiments::all_strategies;
use extremal_strategies::init::paley_graph;
use extremal_worker_api::{CollectingObserver, SearchJob};
use std::collections::HashSet;

fn bench_n5(c: &mut Criterion) {
    let strategies = all_strategies();
    let mut group = c.benchmark_group("R(3,3)/n=5");

    for strategy in &strategies {
        group.bench_function(strategy.id(), |b| {
            b.iter(|| {
                let observer = CollectingObserver::new();
                let job = SearchJob {
                    n: 5,
                    max_iters: 10_000,
                    seed: 42,
                    init_graph: Some(paley_graph(5)),
                    config: serde_json::json!({"target_k": 3, "target_ell": 3}),
                    known_cids: HashSet::new(),
                    max_known_cids: 1000,
                    carry_state: None,
                };
                strategy.search(&job, &observer)
            });
        });
    }
    group.finish();
}

fn bench_n17(c: &mut Criterion) {
    let strategies = all_strategies();
    let mut group = c.benchmark_group("R(4,4)/n=17");
    group.sample_size(10);

    for strategy in &strategies {
        group.bench_function(strategy.id(), |b| {
            b.iter(|| {
                let observer = CollectingObserver::new();
                let job = SearchJob {
                    n: 17,
                    max_iters: 100_000,
                    seed: 42,
                    init_graph: Some(paley_graph(17)),
                    config: serde_json::json!({"target_k": 4, "target_ell": 4}),
                    known_cids: HashSet::new(),
                    max_known_cids: 1000,
                    carry_state: None,
                };
                strategy.search(&job, &observer)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_n5, bench_n17);
criterion_main!(benches);
