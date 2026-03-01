//! Ramsey graph search heuristics and worker logic.
//!
//! Provides greedy construction, local search with tabu, and
//! simulated annealing for finding Ramsey-valid graphs.

pub mod annealing;
pub mod client;
pub mod error;
pub mod greedy;
pub mod init;
pub mod local_search;
pub mod search;
pub mod viz;
pub mod worker;

pub const SEARCH_VERSION: &str = "0.1.0";
