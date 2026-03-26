//! Goodman's formula for minimum monochromatic triangle count.
//!
//! Goodman (1959) proved that for any graph G on n vertices, the total
//! number of monochromatic triangles (triangles in G plus triangles in
//! the complement of G) is at least `goodman_minimum(n)`.
//!
//! Formula: `g(n) = C(n,3) - floor(n * floor((n-1)^2 / 4) / 2)`
//!
//! The minimum is achieved when all vertex degrees equal floor((n-1)/2).
//! Cross-validated against the degree-sum reference implementation in tests.

/// Compute the Goodman minimum for n vertices.
///
/// This is the theoretical minimum number of monochromatic triangles
/// in any 2-coloring of K_n.
///
/// Formula: `g(n) = C(n,3) - floor(n * floor((n-1)^2 / 4) / 2)`
pub fn goodman_minimum(n: u32) -> u64 {
    if n < 3 {
        return 0;
    }
    let n = n as u64;
    let c_n_3 = n * (n - 1) * (n - 2) / 6;
    // Minimum is achieved when all vertex degrees equal floor((n-1)/2).
    let floor_term = n * ((n - 1) * (n - 1) / 4) / 2;
    c_n_3 - floor_term
}

/// Compute the Goodman gap: difference between actual monochromatic
/// triangle count and the theoretical minimum.
///
/// `red_triangles` + `blue_triangles` is the total monochromatic triangle count.
pub fn goodman_gap(n: u32, red_triangles: u64, blue_triangles: u64) -> u64 {
    let actual = red_triangles + blue_triangles;
    let minimum = goodman_minimum(n);
    actual.saturating_sub(minimum)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference implementation: compute exact Goodman minimum via degree-sum.
    /// Used only for cross-validation testing.
    ///
    /// g(n) = C(n,3) - sum(d_v * (n-1-d_v)) / 2
    /// minimized when all d_v = floor((n-1)/2) or ceil((n-1)/2).
    fn goodman_minimum_exact(n: u32) -> u64 {
        if n < 3 {
            return 0;
        }
        let n = n as u64;
        let c_n_3 = n * (n - 1) * (n - 2) / 6;
        let d_low = (n - 1) / 2;
        let d_high = n / 2; // = ceil((n-1)/2)
        let sum = if n % 2 == 1 {
            // All degrees = (n-1)/2 (exact integer)
            n * d_low * (n - 1 - d_low)
        } else {
            // n/2 vertices at d_low, n/2 at d_high
            (n / 2) * d_low * (n - 1 - d_low) + (n / 2) * d_high * (n - 1 - d_high)
        };
        c_n_3 - sum / 2
    }

    /// Cross-validate goodman_minimum() against the exact degree-sum
    /// reference for n = 0..50.
    #[test]
    fn goodman_minimum_cross_validation() {
        for n in 0..50 {
            let fast = goodman_minimum(n);
            let exact = goodman_minimum_exact(n);
            assert_eq!(
                fast, exact,
                "goodman_minimum({n}) = {fast}, expected {exact} (from degree-sum)"
            );
        }
    }

    /// Spot-check specific known values.
    #[test]
    fn goodman_minimum_known_values() {
        assert_eq!(goodman_minimum(0), 0);
        assert_eq!(goodman_minimum(1), 0);
        assert_eq!(goodman_minimum(2), 0);
        assert_eq!(goodman_minimum(3), 0);
        assert_eq!(goodman_minimum(4), 0);
        assert_eq!(goodman_minimum(5), 0);
        assert_eq!(goodman_minimum(6), 2);
        assert_eq!(goodman_minimum(7), 4);
        assert_eq!(goodman_minimum(8), 8);
        assert_eq!(goodman_minimum(9), 12);
        assert_eq!(goodman_minimum(17), 136);
        assert_eq!(goodman_minimum(25), 500);
    }

    #[test]
    fn goodman_gap_works() {
        let n = 10;
        let min = goodman_minimum(n);
        // If actual == minimum, gap is 0
        assert_eq!(goodman_gap(n, min / 2, min - min / 2), 0);
        // If actual > minimum, gap is positive
        assert_eq!(goodman_gap(n, min, 1), 1);
    }

    #[test]
    fn goodman_monotone() {
        let mut prev = 0;
        for n in 0..=50 {
            let g = goodman_minimum(n);
            assert!(g >= prev, "Goodman({n})={g} < Goodman({})={prev}", n - 1);
            prev = g;
        }
    }
}
