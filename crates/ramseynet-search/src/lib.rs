//! Ramsey graph search heuristics and worker logic.
//!
//! Provides greedy construction, local search with tabu, and
//! simulated annealing for finding Ramsey-valid graphs.

pub const SEARCH_VERSION: &str = "0.1.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_exists() {
        assert!(!SEARCH_VERSION.is_empty());
    }
}
