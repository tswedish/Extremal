//! Built-in search strategies for Extremal.
//!
//! Strategies are pure computation implementing the [`SearchStrategy`] trait.
//! They receive a [`SearchJob`] and produce [`SearchResult`] with discovered
//! graphs.
//!
//! ## Strategy parameters
//!
//! Since v1 leaderboards are indexed by `n` only, the Ramsey target `(k, ell)`
//! is passed via the strategy config JSON as `target_k` and `target_ell`.
//! Default: k=5, ell=5 (R(5,5) search).

pub mod crossover;
pub mod init;
pub mod polish;
pub mod tabu;
pub mod tree2;

use extremal_worker_api::SearchStrategy;

/// Get all built-in strategies.
pub fn default_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![
        Box::new(tree2::Tree2Search),
        Box::new(tabu::TabuSearch),
        Box::new(crossover::CrossoverSearch),
    ]
}
