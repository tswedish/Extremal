//! Benchmarks for RamseyNet verifier scoring pipeline.
//!
//! Run with: cargo bench -p ramseynet-verifier
//! Or via:   ./run bench
//!
//! These benchmarks cover the hot path: clique counting, triangle counting,
//! nauty canonical form, full scoring pipeline, and Ramsey verification.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ramseynet_graph::AdjacencyMatrix;
use ramseynet_types::GraphCid;
use ramseynet_verifier::{
    automorphism::canonical_form,
    clique::{count_cliques, count_max_cliques, find_clique_witness},
    scoring::{compute_score_canonical, goodman_minimum},
    verify_ramsey,
};

// ── Graph constructors ──────────────────────────────────────────────

/// C5: cycle on 5 vertices. Valid R(3,3) witness (triangle-free, no ind. set of 3).
fn make_c5() -> AdjacencyMatrix {
    let mut g = AdjacencyMatrix::new(5);
    for i in 0..5 {
        g.set_edge(i, (i + 1) % 5, true);
    }
    g
}

/// Paley graph on prime p: edge (i,j) iff (i-j) is a quadratic residue mod p.
/// Paley(5) ≅ C5. Paley(13) is a valid R(4,4) witness. Paley(17) is the
/// canonical R(4,4) n=17 graph.
fn paley(p: u32) -> AdjacencyMatrix {
    let mut qr = vec![false; p as usize];
    for i in 1..p {
        qr[((i as u64 * i as u64) % p as u64) as usize] = true;
    }
    let mut g = AdjacencyMatrix::new(p);
    for i in 0..p {
        for j in (i + 1)..p {
            let diff = j.abs_diff(i);
            if qr[diff as usize] {
                g.set_edge(i, j, true);
            }
        }
    }
    g
}

/// Random-ish graph on n vertices with ~50% edge density.
/// Uses a simple deterministic hash to be reproducible.
fn dense_random(n: u32) -> AdjacencyMatrix {
    let mut g = AdjacencyMatrix::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            // Simple deterministic hash
            let h = (i.wrapping_mul(31) ^ j.wrapping_mul(97)).wrapping_add(i ^ j);
            if h % 2 == 0 {
                g.set_edge(i, j, true);
            }
        }
    }
    g
}

// ── Benchmarks ──────────────────────────────────────────────────────

fn bench_count_cliques(c: &mut Criterion) {
    let mut group = c.benchmark_group("count_cliques");

    // Triangle counting (k=3) at various sizes — the new Goodman computation
    for &(label, n) in &[
        ("C5", 5u32),
        ("Paley13", 13),
        ("Paley17", 17),
        ("dense25", 25),
    ] {
        let g = match label {
            "C5" => make_c5(),
            "Paley13" => paley(13),
            "Paley17" => paley(17),
            _ => dense_random(n),
        };
        group.bench_with_input(BenchmarkId::new("triangles", label), &g, |b, g| {
            b.iter(|| count_cliques(black_box(g), 3))
        });
    }

    // Triangle counting on complement (for Goodman number)
    for &(label, n) in &[("Paley13", 13u32), ("Paley17", 17), ("dense25", 25)] {
        let g = match label {
            "Paley13" => paley(13),
            "Paley17" => paley(17),
            _ => dense_random(n),
        };
        let comp = g.complement();
        group.bench_with_input(
            BenchmarkId::new("triangles_complement", label),
            &comp,
            |b, g| b.iter(|| count_cliques(black_box(g), 3)),
        );
    }

    // Edge counting (k=2) — baseline
    let p17 = paley(17);
    group.bench_function("edges_Paley17", |b| {
        b.iter(|| count_cliques(black_box(&p17), 2))
    });

    // 4-clique counting
    let d20 = dense_random(20);
    group.bench_function("4cliques_dense20", |b| {
        b.iter(|| count_cliques(black_box(&d20), 4))
    });

    group.finish();
}

fn bench_count_max_cliques(c: &mut Criterion) {
    let mut group = c.benchmark_group("count_max_cliques");

    for &label in &["C5", "Paley13", "Paley17"] {
        let g = match label {
            "C5" => make_c5(),
            "Paley13" => paley(13),
            _ => paley(17),
        };
        group.bench_with_input(BenchmarkId::from_parameter(label), &g, |b, g| {
            b.iter(|| count_max_cliques(black_box(g)))
        });
    }

    // Also on complement (for alpha computation)
    let p17_comp = paley(17).complement();
    group.bench_function("complement_Paley17", |b| {
        b.iter(|| count_max_cliques(black_box(&p17_comp)))
    });

    group.finish();
}

fn bench_find_clique_witness(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_clique_witness");

    let p17 = paley(17);
    let p17_comp = p17.complement();

    // Valid R(4,4) graph — no 4-clique exists, must exhaust search
    group.bench_function("no_4clique_Paley17", |b| {
        b.iter(|| find_clique_witness(black_box(&p17), 4))
    });

    // No 4-independent-set exists either
    group.bench_function("no_4indep_Paley17", |b| {
        b.iter(|| find_clique_witness(black_box(&p17_comp), 4))
    });

    // Has 3-clique (dense graph) — should find it fast
    let d15 = dense_random(15);
    group.bench_function("has_3clique_dense15", |b| {
        b.iter(|| find_clique_witness(black_box(&d15), 3))
    });

    group.finish();
}

fn bench_canonical_form(c: &mut Criterion) {
    let mut group = c.benchmark_group("canonical_form");

    for &label in &["C5", "Paley13", "Paley17"] {
        let g = match label {
            "C5" => make_c5(),
            "Paley13" => paley(13),
            _ => paley(17),
        };
        group.bench_with_input(BenchmarkId::from_parameter(label), &g, |b, g| {
            b.iter(|| canonical_form(black_box(g)))
        });
    }

    let d20 = dense_random(20);
    group.bench_function("dense20", |b| b.iter(|| canonical_form(black_box(&d20))));

    group.finish();
}

fn bench_complement(c: &mut Criterion) {
    let mut group = c.benchmark_group("complement");

    for &(label, n) in &[("n5", 5u32), ("n13", 13), ("n17", 17), ("n25", 25)] {
        let g = if n == 5 { make_c5() } else { dense_random(n) };
        group.bench_with_input(BenchmarkId::from_parameter(label), &g, |b, g| {
            b.iter(|| black_box(g).complement())
        });
    }

    group.finish();
}

fn bench_verify_ramsey(c: &mut Criterion) {
    let mut group = c.benchmark_group("verify_ramsey");

    let dummy_cid = GraphCid([0u8; 32]);

    // Valid R(3,3) n=5 — must prove no 3-clique AND no 3-indep-set
    let c5 = make_c5();
    group.bench_function("valid_R33_C5", |b| {
        b.iter(|| verify_ramsey(black_box(&c5), 3, 3, &dummy_cid))
    });

    // Valid R(4,4) n=17 — much heavier
    let p17 = paley(17);
    group.bench_function("valid_R44_Paley17", |b| {
        b.iter(|| verify_ramsey(black_box(&p17), 4, 4, &dummy_cid))
    });

    // Invalid — has a 3-clique, should short-circuit fast
    let d10 = dense_random(10);
    group.bench_function("invalid_R33_dense10", |b| {
        b.iter(|| verify_ramsey(black_box(&d10), 3, 3, &dummy_cid))
    });

    group.finish();
}

fn bench_compute_score_canonical(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_score_canonical");
    // This is the full scoring pipeline: count_max_cliques (graph + comp),
    // count_cliques(3) for Goodman, canonical_form (nauty), compute_cid.

    let c5 = make_c5();
    group.bench_function("C5", |b| b.iter(|| compute_score_canonical(black_box(&c5))));

    let p13 = paley(13);
    group.bench_function("Paley13", |b| {
        b.iter(|| compute_score_canonical(black_box(&p13)))
    });

    let p17 = paley(17);
    group.bench_function("Paley17", |b| {
        b.iter(|| compute_score_canonical(black_box(&p17)))
    });

    group.finish();
}

fn bench_goodman_minimum(c: &mut Criterion) {
    let mut group = c.benchmark_group("goodman_minimum");

    // Should be trivially fast, but verify no regression
    group.bench_function("n17", |b| b.iter(|| goodman_minimum(black_box(17))));
    group.bench_function("n25", |b| b.iter(|| goodman_minimum(black_box(25))));
    group.bench_function("n100", |b| b.iter(|| goodman_minimum(black_box(100))));

    group.finish();
}

criterion_group!(
    benches,
    bench_count_cliques,
    bench_count_max_cliques,
    bench_find_clique_witness,
    bench_canonical_form,
    bench_complement,
    bench_verify_ramsey,
    bench_compute_score_canonical,
    bench_goodman_minimum,
);
criterion_main!(benches);
