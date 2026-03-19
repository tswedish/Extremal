//! Graph scoring for MineGraph.
//!
//! Implements the full k-clique histogram scoring system:
//! - Count monochromatic k-cliques for k = 3, 4, 5, ... (both colors)
//! - Lexicographic score from highest k down to 3
//! - Goodman gap tiebreaker
//! - Automorphism group size tiebreaker (higher is better)
//! - CID tiebreaker (lower is better)

pub mod automorphism;
pub mod clique;
pub mod goodman;
pub mod histogram;
pub mod score;
