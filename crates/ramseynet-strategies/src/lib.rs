//! Search strategy implementations for RamseyNet.
//!
//! Provides tree/beam search and evolutionary SA strategies.
//! Additional strategies can be added by implementing the
//! [`SearchStrategy`] trait from `ramseynet-worker-api`.

pub mod evo;
pub mod tree;

use ramseynet_worker_api::SearchStrategy;

/// Return all available search strategies.
pub fn default_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![Box::new(tree::TreeSearch), Box::new(evo::EvoSearch)]
}
