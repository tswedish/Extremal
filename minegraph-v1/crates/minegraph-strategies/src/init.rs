//! Seed graph construction for search initialization.

use minegraph_graph::AdjacencyMatrix;

/// Construct a Paley graph on n vertices.
///
/// Uses the smallest prime p >= n with p ≡ 1 (mod 4). Vertices i,j are
/// connected iff (i-j) is a quadratic residue mod p.
///
/// Paley graphs are self-complementary and have high automorphism order,
/// making them excellent seeds for Ramsey search.
pub fn paley_graph(n: u32) -> AdjacencyMatrix {
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

/// Construct a random G(n, 0.5) graph.
pub fn random_graph(n: u32, rng: &mut impl rand::Rng) -> AdjacencyMatrix {
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

/// Apply random noise flips to a graph.
pub fn perturb(graph: &mut AdjacencyMatrix, noise_flips: u32, rng: &mut impl rand::Rng) {
    let n = graph.n();
    for _ in 0..noise_flips {
        let i = rng.gen_range(0..n);
        let j = rng.gen_range(0..n);
        if i != j {
            let cur = graph.edge(i, j);
            graph.set_edge(i, j, !cur);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paley_5_is_c5() {
        let g = paley_graph(5);
        assert_eq!(g.num_edges(), 5);
        // C5: each vertex has degree 2
        for v in 0..5 {
            assert_eq!(g.degree(v), 2);
        }
    }

    #[test]
    fn paley_17_is_regular() {
        let g = paley_graph(17);
        // Paley(17): each vertex has degree 8 = (17-1)/2
        for v in 0..17 {
            assert_eq!(g.degree(v), 8);
        }
    }

    #[test]
    fn paley_self_complementary() {
        // Paley graphs on primes p ≡ 1 (mod 4) are self-complementary
        let g = paley_graph(5);
        let comp = g.complement();
        assert_eq!(g.num_edges(), comp.num_edges());
    }
}
