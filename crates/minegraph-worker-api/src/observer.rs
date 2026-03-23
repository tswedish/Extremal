//! Search observer trait for progress reporting and cancellation.

use minegraph_graph::AdjacencyMatrix;

/// Observer for search progress. Strategies call these methods during
/// execution; the platform provides the implementation.
pub trait SearchObserver: Send + Sync {
    /// Report progress during search. Called periodically (e.g., every 100
    /// iterations) to update visualizations and metrics.
    fn on_progress(&self, info: &ProgressInfo);

    /// Report a valid graph discovered mid-search. The platform will score
    /// and submit it periodically. Strategies should call this immediately
    /// when a valid graph is found.
    fn on_discovery(&self, discovery: &crate::strategy::RawDiscovery) {
        let _ = discovery; // default no-op
    }

    /// Check if the search should be cancelled (e.g., shutdown signal).
    /// Strategies should check this periodically (every ~100 iterations).
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Progress snapshot from a running search.
#[derive(Clone, Debug)]
pub struct ProgressInfo {
    /// The current best graph being explored.
    pub graph: AdjacencyMatrix,
    /// Target vertex count.
    pub n: u32,
    /// Strategy identifier.
    pub strategy: String,
    /// Current iteration number.
    pub iteration: u64,
    /// Maximum iterations budget.
    pub max_iters: u64,
    /// Whether the current best graph is valid (zero violations).
    pub valid: bool,
    /// Violation score of the current best graph (0 = valid).
    pub violation_score: u32,
    /// Number of valid graphs discovered so far.
    pub discoveries_so_far: u64,
}

/// No-op observer for testing.
pub struct NoOpObserver;

impl SearchObserver for NoOpObserver {
    fn on_progress(&self, _info: &ProgressInfo) {}
}

/// Discovery-collecting observer.
///
/// Collects all discovered graphs in a thread-safe `Vec`. Use `drain()` to
/// retrieve and clear collected discoveries. No progress forwarding — use
/// this in benchmarks, tests, or any context where you only care about results.
pub struct CollectingObserver {
    discoveries: std::sync::Mutex<Vec<crate::strategy::RawDiscovery>>,
}

impl CollectingObserver {
    /// Create a new empty collecting observer.
    pub fn new() -> Self {
        Self {
            discoveries: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Drain all collected discoveries, leaving the internal buffer empty.
    pub fn drain(&self) -> Vec<crate::strategy::RawDiscovery> {
        std::mem::take(&mut *self.discoveries.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Default for CollectingObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchObserver for CollectingObserver {
    fn on_progress(&self, _info: &ProgressInfo) {}

    fn on_discovery(&self, discovery: &crate::strategy::RawDiscovery) {
        self.discoveries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(discovery.clone());
    }
}
