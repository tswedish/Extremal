//! Graph representation, graph6 encoding, and content addressing for Extremal.
//!
//! This crate provides:
//! - [`AdjacencyMatrix`] — packed upper-triangular adjacency matrix
//! - [`graph6`] — encode/decode in the standard graph6 format
//! - [`cid`] — content identifiers via blake3 hashing of canonical graph6

pub mod adjacency;
pub mod cid;
pub mod graph6;

pub use adjacency::AdjacencyMatrix;
pub use cid::compute_cid;
pub use graph6::{Graph6Error, GraphJson};
