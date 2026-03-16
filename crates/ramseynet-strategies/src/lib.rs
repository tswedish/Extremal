//! Search strategy implementations for RamseyNet.
//!
//! Provides tree/beam search, incremental beam search, and evolutionary SA.
//! Additional strategies can be added by implementing the
//! [`SearchStrategy`] trait from `ramseynet-worker-api`.

pub mod evo;
pub(crate) mod incremental;
pub mod tree;
pub mod tree2;

use ramseynet_worker_api::SearchStrategy;

/// Return all available search strategies.
pub fn default_strategies() -> Vec<Box<dyn SearchStrategy>> {
    vec![
        Box::new(tree::TreeSearch),
        Box::new(tree2::Tree2Search),
        Box::new(evo::EvoSearch),
    ]
}
