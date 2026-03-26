//! Search strategy trait and job/result types.

use std::any::Any;
use std::collections::HashSet;

use extremal_graph::AdjacencyMatrix;
use extremal_types::GraphCid;

use crate::command::ConfigParam;
use crate::observer::SearchObserver;

/// A search strategy that can be registered, discovered, and invoked.
///
/// Strategies are pure computation — no network, no filesystem, no async.
/// The platform provides everything the strategy needs via [`SearchJob`],
/// and the strategy returns all results via [`SearchResult`].
///
/// # Contract
///
/// - `search()` must check `observer.is_cancelled()` periodically
/// - `search()` must call `observer.on_discovery()` for each valid graph found
/// - The strategy must be deterministic given the same `job.seed`
/// - Strategies must be `Send + Sync + 'static` for use across threads
pub trait SearchStrategy: Send + Sync + 'static {
    /// Unique identifier for this strategy (e.g., "tree2", "evo").
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Describe the configuration parameters this strategy accepts.
    /// Used by the dashboard UI to render dynamic config forms.
    fn config_schema(&self) -> Vec<ConfigParam>;

    /// Execute a search job.
    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult;
}

/// Immutable input to a search job.
///
/// The platform constructs this before each round. Strategies should not
/// need anything beyond what's in this struct.
pub struct SearchJob {
    /// Target vertex count. This is the leaderboard index in Extremal v1.
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

    /// Maximum size for the known CIDs set.
    pub max_known_cids: usize,

    /// Opaque state carried over from the previous round's SearchResult.
    /// None on the first round or after a strategy switch.
    pub carry_state: Option<Box<dyn Any + Send>>,
}

/// Output from a completed search job.
pub struct SearchResult {
    /// The best graph found (may or may not be valid).
    pub best_graph: Option<AdjacencyMatrix>,

    /// Whether any valid graph was found.
    pub valid: bool,

    /// Number of evaluations performed.
    pub iterations_used: u64,

    /// All valid graphs discovered during the search. The platform will
    /// score these, deduplicate by canonical CID, and submit to the server.
    pub discoveries: Vec<RawDiscovery>,

    /// Opaque state to carry over to the next round.
    pub carry_state: Option<Box<dyn Any + Send>>,
}

/// A valid graph discovered during search, before platform scoring.
#[derive(Clone, Debug)]
pub struct RawDiscovery {
    /// The valid graph as discovered (not necessarily in canonical form).
    pub graph: AdjacencyMatrix,
    /// Iteration at which this graph was found.
    pub iteration: u64,
}
