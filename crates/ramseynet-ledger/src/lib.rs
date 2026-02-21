//! Local SQLite ledger for RamseyNet transactions and state.
//!
//! Provides storage and retrieval for challenges, graph submissions,
//! verification receipts, and derived best-known records.

pub const LEDGER_VERSION: &str = "0.1.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_exists() {
        assert!(!LEDGER_VERSION.is_empty());
    }
}
