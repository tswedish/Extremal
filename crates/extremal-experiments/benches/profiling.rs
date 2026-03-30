//! Profiling benchmarks for GPU acceleration planning.
//!
//! Measures individual hot functions to determine where parallelism
//! (rayon or GPU) would have the most impact.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use extremal_scoring::automorphism::canonical_form;
use extremal_scoring::clique::{
    NeighborSet, count_cliques, count_cliques_through_edge, fast_fingerprint, violation_delta,
};
use extremal_strategies::init::paley_graph;

/// Set up a Paley(25) graph with precomputed neighbor masks.
fn setup_n25() -> (NeighborSet, NeighborSet, u32) {
    let graph = paley_graph(25);
    let adj = NeighborSet::from_adj(&graph);
    let comp_graph = graph.complement();
    let comp = NeighborSet::from_adj(&comp_graph);
    (adj, comp, 25)
}

fn bench_violation_delta_single(c: &mut Criterion) {
    let (adj, comp, _n) = setup_n25();
    c.bench_function("violation_delta/single/n25_k5", |b| {
        b.iter(|| violation_delta(black_box(&adj), black_box(&comp), 5, 5, 3, 7));
    });
}

fn bench_violation_delta_all_edges(c: &mut Criterion) {
    let (adj, comp, n) = setup_n25();
    let edges: Vec<(u32, u32)> = (0..n)
        .flat_map(|u| ((u + 1)..n).map(move |v| (u, v)))
        .collect();

    c.bench_function("violation_delta/all_300_edges/n25_k5", |b| {
        b.iter(|| {
            let mut total: i64 = 0;
            for &(u, v) in black_box(&edges) {
                let (dk, de) = violation_delta(&adj, &comp, 5, 5, u, v);
                total += dk + de;
            }
            total
        });
    });
}

fn bench_violation_delta_beam_depth(c: &mut Criterion) {
    let (adj, comp, n) = setup_n25();
    let edges: Vec<(u32, u32)> = (0..n)
        .flat_map(|u| ((u + 1)..n).map(move |v| (u, v)))
        .collect();

    // Simulate 100 parents × 300 edges = 30,000 calls
    // (Same graph for all parents — measures raw call throughput)
    let mut group = c.benchmark_group("violation_delta/beam_depth");
    group.sample_size(10);
    group.bench_function("100_parents_x_300_edges/n25_k5", |b| {
        b.iter(|| {
            let mut total: i64 = 0;
            for _parent in 0..100 {
                for &(u, v) in black_box(&edges) {
                    let (dk, de) = violation_delta(&adj, &comp, 5, 5, u, v);
                    total += dk + de;
                }
            }
            total
        });
    });
    group.finish();
}

fn bench_count_cliques_through_edge(c: &mut Criterion) {
    let (adj, _comp, _n) = setup_n25();
    c.bench_function("count_cliques_through_edge/n25_k5", |b| {
        b.iter(|| count_cliques_through_edge(black_box(&adj), 5, 3, 7));
    });
}

fn bench_count_cliques_full(c: &mut Criterion) {
    let (adj, comp, n) = setup_n25();
    let mut group = c.benchmark_group("count_cliques_full");
    group.sample_size(10);
    group.bench_function("adj_k5/n25", |b| {
        b.iter(|| count_cliques(black_box(&adj), 5, n));
    });
    group.bench_function("comp_k5/n25", |b| {
        b.iter(|| count_cliques(black_box(&comp), 5, n));
    });
    group.finish();
}

fn bench_fast_fingerprint(c: &mut Criterion) {
    let (adj, _comp, _n) = setup_n25();
    c.bench_function("fast_fingerprint/n25", |b| {
        b.iter(|| fast_fingerprint(black_box(&adj.masks)));
    });
}

fn bench_fingerprint_with_flip(c: &mut Criterion) {
    let (adj, _comp, _n) = setup_n25();
    // Simulate computing fingerprint for a flipped edge without mutating original
    c.bench_function("fingerprint_flip_copy/n25", |b| {
        b.iter(|| {
            let mut masks = adj.masks.clone();
            masks[3] ^= 1u64 << 7;
            masks[7] ^= 1u64 << 3;
            fast_fingerprint(black_box(&masks))
        });
    });
}

fn bench_canonical_form(c: &mut Criterion) {
    let graph = paley_graph(25);
    c.bench_function("canonical_form_nauty/n25", |b| {
        b.iter(|| canonical_form(black_box(&graph)));
    });
}

fn bench_canonical_form_plus_cid(c: &mut Criterion) {
    let graph = paley_graph(25);
    c.bench_function("canonical_form+cid/n25", |b| {
        b.iter(|| {
            let (canonical, _) = canonical_form(black_box(&graph));
            extremal_graph::compute_cid(&canonical)
        });
    });
}

fn bench_polish_step_cost(c: &mut Criterion) {
    let (adj, comp, n) = setup_n25();
    // Simulate one polish step: 300 edges × 3 violation_delta + 1 canonical_form
    let graph = paley_graph(25);
    let edges: Vec<(u32, u32)> = (0..n)
        .flat_map(|u| ((u + 1)..n).map(move |v| (u, v)))
        .collect();

    let mut group = c.benchmark_group("polish_step");
    group.bench_function("edge_eval_only/n25", |b| {
        b.iter(|| {
            for &(u, v) in black_box(&edges) {
                let (dk, de) = violation_delta(&adj, &comp, 5, 5, u, v);
                if dk + de != 0 {
                    continue;
                }
                violation_delta(&adj, &comp, 4, 4, u, v);
                violation_delta(&adj, &comp, 3, 3, u, v);
            }
        });
    });
    group.bench_function("canonical_form_only/n25", |b| {
        b.iter(|| {
            let (canonical, _) = canonical_form(black_box(&graph));
            extremal_graph::compute_cid(&canonical)
        });
    });
    group.finish();
}

criterion_group!(
    profiling,
    bench_violation_delta_single,
    bench_violation_delta_all_edges,
    bench_violation_delta_beam_depth,
    bench_count_cliques_through_edge,
    bench_count_cliques_full,
    bench_fast_fingerprint,
    bench_fingerprint_with_flip,
    bench_canonical_form,
    bench_canonical_form_plus_cid,
    bench_polish_step_cost,
);
criterion_main!(profiling);
