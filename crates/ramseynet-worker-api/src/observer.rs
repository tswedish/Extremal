//! Search observer trait for progress reporting and cancellation.

/// Observer for search progress. Strategies call these methods during
/// execution; the platform provides the implementation.
pub trait SearchObserver: Send + Sync {
    /// Report progress during search. Called periodically (e.g., every 100
    /// iterations) to update visualizations and metrics.
    fn on_progress(&self, info: &ProgressInfo);

    /// Report a valid graph discovered mid-search. The platform will score
    /// and submit it periodically. Strategies should call this immediately
    /// when a valid graph is found, rather than waiting until search ends.
    ///
    /// The graph does NOT need to be in canonical form — the platform handles
    /// canonicalization and dedup.
    fn on_discovery(&self, discovery: &crate::strategy::RawDiscovery) {
        let _ = discovery; // default no-op
    }

    /// Check if the search should be cancelled (e.g., shutdown signal).
    /// Strategies should check this periodically (every ~100 iterations).
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Progress snapshot from a running search. Owned (no lifetimes) for
/// easy passing across thread boundaries.
#[derive(Clone, Debug)]
pub struct ProgressInfo {
    /// The current best graph being explored.
    pub graph: ramseynet_graph::AdjacencyMatrix,
    /// Target vertex count.
    pub n: u32,
    /// Ramsey parameter k.
    pub k: u32,
    /// Ramsey parameter ell.
    pub ell: u32,
    /// Strategy identifier.
    pub strategy: String,
    /// Current iteration number.
    pub iteration: u64,
    /// Maximum iterations budget.
    pub max_iters: u64,
    /// Whether the current best graph is valid.
    pub valid: bool,
    /// Violation score of the current best graph (0 = valid).
    pub violation_score: u32,
    /// Number of valid graphs discovered so far.
    pub discoveries_so_far: u64,
    /// Number of k-cliques in current best (optional detail).
    pub k_cliques: Option<u64>,
    /// Number of ell-independent sets in current best (optional detail).
    pub ell_indsets: Option<u64>,
}

/// No-op observer for testing and when no visualization is needed.
pub struct NoOpObserver;

impl SearchObserver for NoOpObserver {
    fn on_progress(&self, _info: &ProgressInfo) {}
}
