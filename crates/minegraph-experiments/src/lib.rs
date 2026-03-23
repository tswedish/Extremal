//! Experiment strategies and benchmark harness for MineGraph.
//!
//! This crate is **not** depended on by any production crate. It exists for
//! fast iteration on new search strategies without touching production code.
//!
//! # Usage
//!
//! ```bash
//! cargo run -p minegraph-experiments --release -- compare --n 25 --budget 100000 --seeds 10
//! ```
//!
//! # Adding a new strategy
//!
//! 1. Create `src/my_strategy.rs` implementing [`SearchStrategy`]
//! 2. Register it in [`experiment_strategies()`]
//! 3. Run the harness and compare against tree2

pub mod harness;
pub mod sa;

use minegraph_strategies::default_strategies;
use minegraph_worker_api::SearchStrategy;

/// Get all experiment strategies (does NOT include production strategies).
pub fn experiment_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![Box::new(sa::SimulatedAnnealing)]
}

/// Get all strategies: production + experimental.
pub fn all_strategies() -> Vec<Box<dyn SearchStrategy>> {
    let mut strategies = default_strategies();
    strategies.extend(experiment_strategies());
    strategies
}
