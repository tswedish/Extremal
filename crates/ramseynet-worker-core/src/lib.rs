//! Worker platform core: engine orchestration, leaderboard sync,
//! submission pipeline, and graph initialization.
//!
//! This crate provides the runtime engine that coordinates search
//! strategies, manages server interaction, and handles the full
//! discovery-to-submission lifecycle.

pub mod client;
pub mod engine;
pub mod error;
pub mod init;

pub use engine::{EngineConfig, WorkerEngine};
pub use error::WorkerError;
pub use init::InitMode;
pub use ramseynet_worker_api::{WorkerCommand, WorkerEvent};

pub const WORKER_VERSION: &str = "0.2.0";

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_verifier::scoring::GraphScore;
use ramseynet_worker_api::ProgressInfo;

/// Trait for the engine to forward events to the worker web-app.
/// The worker crate implements this over VizHandle.
pub trait VizBridge: Send + Sync + 'static {
    /// Forward a progress snapshot from the running search.
    fn on_progress(&self, graph: &AdjacencyMatrix, info: &ProgressInfo);

    /// Forward a scored discovery to the viz leaderboard.
    fn on_discovery(
        &self,
        graph: &AdjacencyMatrix,
        n: u32,
        strategy: &str,
        iteration: u64,
        score: GraphScore,
    );
}
