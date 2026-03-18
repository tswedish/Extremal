//! Graph scoring: the full lexicographic comparison for leaderboard ranking.
//!
//! The MineGraph scoring system is a golf-style ranking where lower is better.
//! The score is a lexicographic tuple built from the clique histogram:
//!
//! 1. For each k from max_k down to 3: `(max(red_k, blue_k), min(red_k, blue_k))`
//! 2. Goodman gap (distance from theoretical minimum 3-clique count)
//! 3. Inverse automorphism: `1/|Aut(G)|` (lower = more symmetric = better)
//! 4. CID bytes (deterministic tiebreaker, lower is better)
//!
//! This module defines [`GraphScore`] with a custom [`Ord`] implementation
//! that encodes this comparison.

use minegraph_types::GraphCid;
use serde::{Deserialize, Serialize};

use crate::histogram::CliqueHistogram;

/// A scored graph entry, containing all components needed for ranking.
///
/// Implements `Ord` with the lexicographic comparison described above.
/// Lower score = better rank.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphScore {
    /// The full clique histogram.
    pub histogram: CliqueHistogram,

    /// Goodman gap: actual monochromatic 3-cliques minus the theoretical minimum.
    pub goodman_gap: u64,

    /// Automorphism group order |Aut(G)|. Higher = more symmetric = better.
    pub aut_order: f64,

    /// Content identifier (blake3 of canonical graph6).
    pub cid: GraphCid,
}

/// The tier-level comparison tuple for one k value: (max, min) of (red, blue).
fn tier_key(red: u64, blue: u64) -> (u64, u64) {
    if red >= blue {
        (red, blue)
    } else {
        (blue, red)
    }
}

impl GraphScore {
    /// Compute a score from a histogram, automorphism order, and CID.
    ///
    /// Note: This does not compute the histogram or aut_order — those must
    /// be provided. See `minegraph-scoring` crate for full computation.
    pub fn new(
        histogram: CliqueHistogram,
        goodman_gap: u64,
        aut_order: f64,
        cid: GraphCid,
    ) -> Self {
        Self {
            histogram,
            goodman_gap,
            aut_order,
            cid,
        }
    }

    /// Serialize the score to bytes for database storage and comparison.
    ///
    /// `max_k` must be consistent across all scores being compared (e.g.,
    /// all scores in the same leaderboard must use the same max_k).
    /// The byte encoding preserves the lexicographic ordering when max_k
    /// is consistent.
    pub fn to_score_bytes(&self, max_k: u32) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Encode tiers from max_k down to 3
        for k in (3..=max_k).rev() {
            let (hi, lo) = if let Some(tier) = self.histogram.tier(k) {
                tier_key(tier.red, tier.blue)
            } else {
                (0, 0)
            };
            bytes.extend_from_slice(&hi.to_be_bytes());
            bytes.extend_from_slice(&lo.to_be_bytes());
        }

        // Goodman gap (lower is better)
        bytes.extend_from_slice(&self.goodman_gap.to_be_bytes());

        // Inverse aut_order: 1/|Aut(G)|. Lower = more symmetric = better.
        // Encode as f64 big-endian. Since aut_order >= 1, inverse is in (0, 1].
        let inv_aut = if self.aut_order > 0.0 {
            1.0 / self.aut_order
        } else {
            1.0
        };
        bytes.extend_from_slice(&inv_aut.to_be_bytes());

        // CID (lower is better, deterministic tiebreaker)
        bytes.extend_from_slice(self.cid.as_bytes());

        bytes
    }
}

impl PartialEq for GraphScore {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for GraphScore {}

impl PartialOrd for GraphScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GraphScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare tiers from highest k down to 3
        let max_k = self
            .histogram
            .max_k()
            .unwrap_or(2)
            .max(other.histogram.max_k().unwrap_or(2));

        for k in (3..=max_k).rev() {
            let self_tier = self
                .histogram
                .tier(k)
                .map(|t| tier_key(t.red, t.blue))
                .unwrap_or((0, 0));
            let other_tier = other
                .histogram
                .tier(k)
                .map(|t| tier_key(t.red, t.blue))
                .unwrap_or((0, 0));
            let cmp = self_tier.cmp(&other_tier);
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }

        // Goodman gap (lower is better)
        let cmp = self.goodman_gap.cmp(&other.goodman_gap);
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }

        // Automorphism order (higher is better = inverse is lower)
        // Note: we compare in reverse because higher aut_order is better
        let cmp = other
            .aut_order
            .partial_cmp(&self.aut_order)
            .unwrap_or(std::cmp::Ordering::Equal);
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }

        // CID tiebreaker (lower is better)
        self.cid.cmp(&other.cid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::histogram::{CliqueHistogram, HistogramTier};

    fn make_score(tiers: Vec<(u32, u64, u64)>, goodman_gap: u64, aut_order: f64) -> GraphScore {
        let hist = CliqueHistogram {
            tiers: tiers
                .into_iter()
                .map(|(k, red, blue)| HistogramTier { k, red, blue })
                .collect(),
            n: 25,
        };
        let cid = GraphCid::from_bytes([0u8; 32]);
        GraphScore::new(hist, goodman_gap, aut_order, cid)
    }

    #[test]
    fn fewer_high_k_cliques_wins() {
        // Graph A: 1 5-clique, 10 triangles
        let a = make_score(vec![(3, 10, 10), (5, 1, 0)], 0, 1.0);
        // Graph B: 0 5-cliques, 100 triangles
        let b = make_score(vec![(3, 100, 100)], 0, 1.0);
        // B is better (lower) because 0 5-cliques < 1 5-clique
        assert!(b < a);
    }

    #[test]
    fn goodman_gap_breaks_tie() {
        let a = make_score(vec![(3, 10, 10)], 5, 1.0);
        let b = make_score(vec![(3, 10, 10)], 3, 1.0);
        assert!(b < a); // lower gap wins
    }

    #[test]
    fn higher_symmetry_wins() {
        let a = make_score(vec![(3, 10, 10)], 0, 1.0);
        let b = make_score(vec![(3, 10, 10)], 0, 25.0); // 25x more symmetric
        assert!(b < a); // higher aut_order = lower inverse = wins
    }

    #[test]
    fn cid_tiebreaker() {
        let hist = CliqueHistogram {
            tiers: vec![HistogramTier {
                k: 3,
                red: 10,
                blue: 10,
            }],
            n: 25,
        };
        let a = GraphScore::new(hist.clone(), 0, 1.0, GraphCid::from_bytes([0u8; 32]));
        let b = GraphScore::new(hist, 0, 1.0, GraphCid::from_bytes([1u8; 32]));
        assert!(a < b); // lower CID wins
    }

    #[test]
    fn symmetric_cliques_normalized() {
        // (red=5, blue=3) and (red=3, blue=5) should score the same
        let a = make_score(vec![(3, 5, 3)], 0, 1.0);
        let b = make_score(vec![(3, 3, 5)], 0, 1.0);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn score_bytes_preserve_ordering() {
        let a = make_score(vec![(3, 10, 10), (5, 1, 0)], 0, 1.0);
        let b = make_score(vec![(3, 100, 100)], 0, 1.0);
        // b < a in GraphScore ordering
        assert!(b < a);
        // Same ordering in byte comparison (must use consistent max_k)
        assert!(b.to_score_bytes(5) < a.to_score_bytes(5));
    }
}
