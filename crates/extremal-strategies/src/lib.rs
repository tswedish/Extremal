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

pub mod cayley;
pub mod circulant;
pub mod construct;
pub mod crossover;
pub mod init;
pub mod lns;
pub mod polish;
pub mod relink;
pub mod sa;
pub mod tabu;
pub mod tree2;

use extremal_worker_api::SearchStrategy;

/// Get all built-in strategies.
pub fn default_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![
        Box::new(tree2::Tree2Search),
        Box::new(tabu::TabuSearch),
        Box::new(crossover::CrossoverSearch),
        Box::new(sa::SimulatedAnnealing),
        Box::new(construct::ConstructSearch),
        Box::new(circulant::CirculantSearch),
        Box::new(cayley::CayleySearch),
        Box::new(lns::LnsSearch),
        Box::new(relink::RelinkSearch),
    ]
}
