//! Search strategy trait and job/result types.

use std::collections::HashSet;

use ramseynet_graph::AdjacencyMatrix;
use ramseynet_types::GraphCid;

use crate::command::ConfigParam;
use crate::observer::SearchObserver;

/// A search strategy that can be registered, discovered, and invoked.
///
/// Strategies are pure computation — no network, no filesystem, no async.
/// The platform provides everything the strategy needs via [`SearchJob`],
/// and the strategy returns all results via [`SearchResult`].
pub trait SearchStrategy: Send + Sync + 'static {
    /// Unique identifier for this strategy (e.g., "tree", "annealing-v2").
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Describe the configuration parameters this strategy accepts.
    /// Used by the control UI to render dynamic config forms.
    fn config_schema(&self) -> Vec<ConfigParam>;

    /// Execute a search job. Must be deterministic given the same job seed.
    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult;
}

/// Immutable input to a search job, fully describing the work to be done.
///
/// The platform constructs this before spawning the search. Strategies
/// should not need anything beyond what's in this struct.
#[derive(Clone, Debug)]
pub struct SearchJob {
    /// Ramsey parameter k (clique size).
    pub k: u32,
    /// Ramsey parameter ell (independent set size).
    pub ell: u32,
    /// Target vertex count.
    pub n: u32,
    /// Maximum iterations (evaluation budget).
    pub max_iters: u64,
    /// Deterministic RNG seed. Strategy creates its own RNG from this.
    pub seed: u64,
    /// Platform-provided seed graph (from leaderboard pool, Paley, etc.).
    /// If None, the strategy should generate its own initial graph.
    pub init_graph: Option<AdjacencyMatrix>,
    /// Strategy-specific configuration (validated by the strategy).
    pub config: serde_json::Value,
    /// Known canonical CIDs from prior rounds/server. Strategies can use
    /// this to avoid re-exploring graphs already on the leaderboard.
    pub known_cids: HashSet<GraphCid>,
    /// Maximum size for the known CIDs set. Strategies should respect
    /// this when the set grows beyond this size during search.
    pub max_known_cids: usize,
}

/// Output from a completed search job.
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// The best graph found (may or may not be valid).
    pub best_graph: Option<AdjacencyMatrix>,
    /// Whether any valid Ramsey graph was found.
    pub valid: bool,
    /// Number of evaluations performed.
    pub iterations_used: u64,
    /// All valid graphs discovered during the search. The platform will
    /// score these (compute_score_canonical), deduplicate by canonical
    /// CID, and submit to the server.
    pub discoveries: Vec<RawDiscovery>,
}

/// A valid graph discovered during search, before platform scoring.
///
/// The strategy provides the raw graph; the platform handles canonical
/// form computation, scoring, and CID derivation.
#[derive(Clone, Debug)]
pub struct RawDiscovery {
    /// The valid graph as discovered (not necessarily in canonical form).
    pub graph: AdjacencyMatrix,
    /// Iteration at which this graph was found.
    pub iteration: u64,
}
