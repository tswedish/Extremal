//! Built-in search strategies for MineGraph.
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

pub mod init;
pub mod tree2;

use minegraph_worker_api::SearchStrategy;

/// Get all built-in strategies.
pub fn default_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![Box::new(tree2::Tree2Search)]
}
