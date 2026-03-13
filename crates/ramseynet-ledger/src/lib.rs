//! Local SQLite ledger for RamseyNet transactions and state.
//!
//! Provides storage and retrieval for graph submissions,
//! verification receipts, and leaderboards.

pub mod error;
pub mod models;
pub mod queries;
mod schema;

pub use error::LedgerError;
pub use models::*;
pub use queries::{AdmitScore, ThresholdInfo};

use rusqlite::Connection;
use std::sync::Mutex;

pub const LEDGER_VERSION: &str = "0.2.0";

/// SQLite-backed ledger for the RamseyNet protocol.
///
/// Thread-safe via `Mutex<Connection>`. Each query acquires the lock,
/// executes, and releases it promptly.
pub struct Ledger {
    conn: Mutex<Connection>,
}

impl Ledger {
    /// Open (or create) a SQLite database at the given path and initialize the schema.
    pub fn open(path: &str) -> Result<Self, LedgerError> {
        let conn = Connection::open(path)?;
        schema::init_db(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (for tests).
    pub fn open_in_memory() -> Result<Self, LedgerError> {
        let conn = Connection::open_in_memory()?;
        schema::init_db(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_exists() {
        assert!(!LEDGER_VERSION.is_empty());
    }

    #[test]
    fn open_in_memory_initializes_schema() {
        let ledger = Ledger::open_in_memory().unwrap();
        let conn = ledger.conn.lock().unwrap();
        // Verify leaderboard table exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='leaderboard'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn submission_receipt_lifecycle() {
        let ledger = Ledger::open_in_memory().unwrap();

        // Store submission
        let sub = ledger
            .store_submission(3, 3, "cid_abc", 5, r#"{"n":5}"#)
            .unwrap();
        assert_eq!(sub.graph_cid, "cid_abc");
        assert_eq!(sub.k, 3);
        assert_eq!(sub.ell, 3);

        // Duplicate fails
        assert!(ledger
            .store_submission(3, 3, "cid_abc", 5, r#"{"n":5}"#)
            .is_err());

        // Store receipt
        let receipt = ledger
            .store_receipt("cid_abc", 3, 3, "accepted", None, None)
            .unwrap();
        assert_eq!(receipt.verdict, "accepted");
    }

    #[test]
    fn leaderboard_admission() {
        let ledger = Ledger::open_in_memory().unwrap();

        // Store a submission first (FK constraint)
        ledger
            .store_submission(3, 3, "cid_a", 5, r#"{"n":5}"#)
            .unwrap();

        let score = AdmitScore {
            tier1_max: 5,
            tier1_min: 5,
            tier2_aut: 10.0,
            tier3_cid: "cid_a".to_string(),
            score_json: "{}".to_string(),
        };

        // Admit to leaderboard
        let entry = ledger
            .try_admit(3, 3, 5, "cid_a", &score)
            .unwrap()
            .expect("should be admitted");
        assert_eq!(entry.rank, 1);
        assert_eq!(entry.graph_cid, "cid_a");

        // Duplicate admission returns existing entry
        let dup = ledger
            .try_admit(3, 3, 5, "cid_a", &score)
            .unwrap()
            .expect("dup should return existing");
        assert_eq!(dup.rank, 1);

        // Better score gets rank 1
        ledger
            .store_submission(3, 3, "cid_b", 5, r#"{"n":5}"#)
            .unwrap();
        let better_score = AdmitScore {
            tier1_max: 3,
            tier1_min: 3,
            tier2_aut: 20.0,
            tier3_cid: "cid_b".to_string(),
            score_json: "{}".to_string(),
        };
        let entry_b = ledger
            .try_admit(3, 3, 5, "cid_b", &better_score)
            .unwrap()
            .expect("should be admitted");
        assert_eq!(entry_b.rank, 1);

        // Original should now be rank 2
        let entries = ledger.get_leaderboard(3, 3, 5).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].graph_cid, "cid_b");
        assert_eq!(entries[0].rank, 1);
        assert_eq!(entries[1].graph_cid, "cid_a");
        assert_eq!(entries[1].rank, 2);
    }

    #[test]
    fn canonical_k_ell() {
        let ledger = Ledger::open_in_memory().unwrap();

        // Submit with k=4, ell=3 — should be stored as k=3, ell=4
        let sub = ledger
            .store_submission(4, 3, "cid_x", 5, r#"{"n":5}"#)
            .unwrap();
        assert_eq!(sub.k, 3);
        assert_eq!(sub.ell, 4);
    }

    #[test]
    fn threshold_info() {
        let ledger = Ledger::open_in_memory().unwrap();

        // Empty board — not full
        let info = ledger.get_threshold(3, 3, 5).unwrap();
        assert_eq!(info.entry_count, 0);
        assert!(info.worst_tier1_max.is_none());
    }

    #[test]
    fn list_leaderboards_and_n_values() {
        let ledger = Ledger::open_in_memory().unwrap();

        ledger
            .store_submission(3, 3, "cid_1", 5, r#"{"n":5}"#)
            .unwrap();
        let score = AdmitScore {
            tier1_max: 5,
            tier1_min: 5,
            tier2_aut: 10.0,
            tier3_cid: "cid_1".to_string(),
            score_json: "{}".to_string(),
        };
        ledger.try_admit(3, 3, 5, "cid_1", &score).unwrap();

        let summaries = ledger.list_leaderboards().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].k, 3);
        assert_eq!(summaries[0].n, 5);
        assert_eq!(summaries[0].entry_count, 1);

        let ns = ledger.list_n_for_pair(3, 3).unwrap();
        assert_eq!(ns, vec![5]);
    }

}
