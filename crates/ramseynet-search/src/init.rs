//! Graph initialization strategies for Ramsey search.
//!
//! Starting from a good initial graph is critical for finding valid Ramsey
//! graphs at large n. A random G(n, 1/2) graph has ~C(n,k)/2^C(k,2) expected
//! k-cliques, which grows fast — by n=15 for R(4,4), the probability of a
//! random graph being valid is ~10^-19. Structured initializations start
//! much closer to the valid region.

use std::sync::{Arc, Mutex};

use rand::rngs::SmallRng;
use rand::Rng;
use ramseynet_graph::AdjacencyMatrix;

/// Available graph initialization strategies.
#[derive(Clone, Debug)]
pub enum InitStrategy {
    /// Uniform random: each edge present with probability 0.5.
    /// Works for easy instances (small n relative to R(k,ell)).
    Random,

    /// Paley-inspired construction: edges based on quadratic residues.
    /// For prime n ≡ 1 (mod 4), produces the true Paley graph (self-complementary,
    /// excellent Ramsey properties). For other n, uses the induced subgraph of
    /// Paley(p) where p is the smallest suitable prime >= n.
    Paley,

    /// Perturbed Paley: start from Paley construction, then randomly flip
    /// a small fraction of edges. Combines algebraic structure with diversity
    /// across restarts. Good default for hard instances.
    PerturbedPaley {
        /// Fraction of edges to randomly flip (0.0 to 1.0).
        flip_fraction: f64,
    },

    /// Balanced random: each edge present with a tuned density.
    /// Useful when the optimal density is known to differ from 0.5.
    BalancedRandom {
        /// Edge probability (0.0 to 1.0).
        density: f64,
    },

    /// Seed from server leaderboard entries. Picks a random graph from the
    /// shared pool each invocation, optionally applying random edge flips as
    /// noise. Falls back to PerturbedPaley if the pool is empty.
    ///
    /// The pool is behind `Arc<Mutex<>>` so the worker can refresh it each
    /// round while searchers read from it concurrently.
    Leaderboard {
        /// Shared pool of graphs, updated by the worker loop.
        pool: Arc<Mutex<Vec<AdjacencyMatrix>>>,
        /// Number of random edge flips to apply as noise (0 = use as-is).
        noise_flips: u32,
    },
}

impl Default for InitStrategy {
    fn default() -> Self {
        InitStrategy::PerturbedPaley {
            flip_fraction: 0.05,
        }
    }
}

/// Generate an initial graph using the given strategy.
pub fn init_graph(n: u32, strategy: &InitStrategy, rng: &mut SmallRng) -> AdjacencyMatrix {
    match strategy {
        InitStrategy::Random => random_graph(n, rng),
        InitStrategy::Paley => paley_graph(n),
        InitStrategy::PerturbedPaley { flip_fraction } => {
            let mut g = paley_graph(n);
            perturb(&mut g, *flip_fraction, rng);
            g
        }
        InitStrategy::BalancedRandom { density } => {
            let mut g = AdjacencyMatrix::new(n);
            for i in 0..n {
                for j in (i + 1)..n {
                    if rng.gen_bool(*density) {
                        g.set_edge(i, j, true);
                    }
                }
            }
            g
        }
        InitStrategy::Leaderboard { pool, noise_flips } => {
            let graphs = pool.lock().unwrap();
            if graphs.is_empty() {
                drop(graphs);
                // Fallback to perturbed Paley if no leaderboard graphs available
                let mut g = paley_graph(n);
                perturb(&mut g, 0.05, rng);
                g
            } else {
                let mut g = graphs[rng.gen_range(0..graphs.len())].clone();
                drop(graphs);
                // Apply noise flips
                if *noise_flips > 0 {
                    flip_random_edges(&mut g, *noise_flips, rng);
                }
                g
            }
        }
    }
}

/// Flip exactly `count` randomly chosen edges.
fn flip_random_edges(graph: &mut AdjacencyMatrix, count: u32, rng: &mut SmallRng) {
    let n = graph.n();
    let num_possible = n * (n - 1) / 2;
    if num_possible == 0 {
        return;
    }
    for _ in 0..count {
        // Pick a random edge position
        let idx = rng.gen_range(0..num_possible);
        // Convert linear index to (i, j)
        let (i, j) = linear_to_edge(idx, n);
        let current = graph.edge(i, j);
        graph.set_edge(i, j, !current);
    }
}

/// Convert a linear index into an upper-triangular edge (i, j) with i < j.
fn linear_to_edge(idx: u32, n: u32) -> (u32, u32) {
    // Row i has (n - 1 - i) entries. Walk rows until we find the right one.
    let mut remaining = idx;
    for i in 0..n {
        let row_len = n - 1 - i;
        if remaining < row_len {
            return (i, i + 1 + remaining);
        }
        remaining -= row_len;
    }
    unreachable!()
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

/// Randomly flip a fraction of the edges.
fn perturb(graph: &mut AdjacencyMatrix, flip_fraction: f64, rng: &mut SmallRng) {
    let n = graph.n();
    for i in 0..n {
        for j in (i + 1)..n {
            if rng.gen_bool(flip_fraction) {
                let current = graph.edge(i, j);
                graph.set_edge(i, j, !current);
            }
        }
    }
}

/// Construct a Paley graph or Paley-induced subgraph on n vertices.
///
/// For prime p ≡ 1 (mod 4), the Paley graph connects vertices i and j
/// iff (i - j) mod p is a quadratic residue. These graphs are
/// self-complementary and have excellent Ramsey properties.
///
/// The Paley(17) graph is the unique R(4,4)-valid graph on 17 vertices.
/// Paley(13) is valid for R(4,4) on 13 vertices.
///
/// For non-prime n or n ≢ 1 (mod 4), we build Paley(p) for the smallest
/// suitable prime p >= n and take the induced subgraph on vertices 0..n-1.
fn paley_graph(n: u32) -> AdjacencyMatrix {
    let p = smallest_paley_prime(n);
    let qr = quadratic_residues(p);

    let mut g = AdjacencyMatrix::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            let diff = ((i as i64 - j as i64).rem_euclid(p as i64)) as u32;
            if qr.contains(&diff) {
                g.set_edge(i, j, true);
            }
        }
    }
    g
}

/// Find the smallest prime p >= n where p ≡ 1 (mod 4).
fn smallest_paley_prime(n: u32) -> u32 {
    let mut p = n.max(5);
    loop {
        if p % 4 == 1 && is_prime(p) {
            return p;
        }
        p += 1;
    }
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

/// Compute the set of quadratic residues mod p (for prime p).
fn quadratic_residues(p: u32) -> Vec<u32> {
    let mut qr = vec![false; p as usize];
    for x in 1..p {
        let r = ((x as u64 * x as u64) % p as u64) as u32;
        qr[r as usize] = true;
    }
    (1..p).filter(|&i| qr[i as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use ramseynet_graph::compute_cid;
    use ramseynet_verifier::verify_ramsey;

    #[test]
    fn paley_17_is_valid_r44() {
        let g = paley_graph(17);
        assert_eq!(g.n(), 17);
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 4, 4, &cid);
        assert_eq!(result.verdict, ramseynet_types::Verdict::Accepted,
            "Paley(17) should be valid for R(4,4)");
    }

    #[test]
    fn paley_13_is_valid_r44() {
        let g = paley_graph(13);
        assert_eq!(g.n(), 13);
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 4, 4, &cid);
        assert_eq!(result.verdict, ramseynet_types::Verdict::Accepted,
            "Paley(13) should be valid for R(4,4)");
    }

    #[test]
    fn paley_5_is_valid_r33() {
        let g = paley_graph(5);
        assert_eq!(g.n(), 5);
        // Paley(5) is C5
        let cid = compute_cid(&g);
        let result = verify_ramsey(&g, 3, 3, &cid);
        assert_eq!(result.verdict, ramseynet_types::Verdict::Accepted,
            "Paley(5) should be valid for R(3,3)");
    }

    #[test]
    fn paley_subgraph_15_from_17() {
        // n=15 should use Paley(17) restricted to vertices 0..14
        let p = smallest_paley_prime(15);
        assert_eq!(p, 17);
        let g = paley_graph(15);
        assert_eq!(g.n(), 15);
    }

    #[test]
    fn perturbed_paley_varies() {
        let mut rng1 = SmallRng::seed_from_u64(1);
        let mut rng2 = SmallRng::seed_from_u64(2);
        let g1 = init_graph(15, &InitStrategy::PerturbedPaley { flip_fraction: 0.1 }, &mut rng1);
        let g2 = init_graph(15, &InitStrategy::PerturbedPaley { flip_fraction: 0.1 }, &mut rng2);
        // Different seeds should produce different graphs
        let mut same = true;
        for i in 0..15 {
            for j in (i + 1)..15 {
                if g1.edge(i, j) != g2.edge(i, j) {
                    same = false;
                }
            }
        }
        assert!(!same, "different seeds should produce different perturbed graphs");
    }

    #[test]
    fn smallest_paley_prime_values() {
        assert_eq!(smallest_paley_prime(5), 5);   // 5 ≡ 1 (mod 4), prime
        assert_eq!(smallest_paley_prime(6), 13);   // next: 13
        assert_eq!(smallest_paley_prime(14), 17);  // next: 17
        assert_eq!(smallest_paley_prime(17), 17);
        assert_eq!(smallest_paley_prime(18), 29);
    }

    #[test]
    fn linear_to_edge_covers_all() {
        let n = 5u32;
        let num_edges = n * (n - 1) / 2;
        let mut edges = Vec::new();
        for idx in 0..num_edges {
            let (i, j) = linear_to_edge(idx, n);
            assert!(i < j, "edge ({i},{j}) should have i < j");
            assert!(j < n, "edge ({i},{j}) should have j < n");
            edges.push((i, j));
        }
        assert_eq!(edges.len(), 10);
        // Should cover all unique edges
        let unique: std::collections::HashSet<_> = edges.into_iter().collect();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn leaderboard_init_picks_from_pool() {
        let mut rng = SmallRng::seed_from_u64(42);
        // Create a pool with a known graph (C5)
        let c5 = paley_graph(5);
        let pool = Arc::new(Mutex::new(vec![c5.clone()]));
        let strategy = InitStrategy::Leaderboard { pool, noise_flips: 0 };
        let g = init_graph(5, &strategy, &mut rng);
        // With no noise and only one graph in pool, output should match
        assert_eq!(g.num_edges(), c5.num_edges());
        for i in 0..5 {
            for j in (i + 1)..5 {
                assert_eq!(g.edge(i, j), c5.edge(i, j));
            }
        }
    }

    #[test]
    fn leaderboard_init_applies_noise() {
        let mut rng = SmallRng::seed_from_u64(42);
        let c5 = paley_graph(5);
        let pool = Arc::new(Mutex::new(vec![c5.clone()]));
        let strategy = InitStrategy::Leaderboard { pool, noise_flips: 3 };
        let g = init_graph(5, &strategy, &mut rng);
        // With 3 noise flips, graph should differ from C5
        let mut diffs = 0;
        for i in 0..5 {
            for j in (i + 1)..5 {
                if g.edge(i, j) != c5.edge(i, j) {
                    diffs += 1;
                }
            }
        }
        assert!(diffs > 0 && diffs <= 3, "expected 1-3 diffs, got {diffs}");
    }

    #[test]
    fn leaderboard_init_falls_back_when_empty() {
        let mut rng = SmallRng::seed_from_u64(42);
        let pool = Arc::new(Mutex::new(Vec::new()));
        let strategy = InitStrategy::Leaderboard { pool, noise_flips: 0 };
        let g = init_graph(5, &strategy, &mut rng);
        // Should fall back to perturbed Paley — just check it produces a valid graph
        assert_eq!(g.n(), 5);
    }
}
