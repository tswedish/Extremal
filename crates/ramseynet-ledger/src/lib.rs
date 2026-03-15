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

pub const LEDGER_VERSION: &str = "0.3.0";

/// Default leaderboard capacity per (k, ell, n) triple.
pub const DEFAULT_LEADERBOARD_CAPACITY: u32 = 500;

/// SQLite-backed ledger for the RamseyNet protocol.
///
/// Thread-safe via `Mutex<Connection>`. Each query acquires the lock,
/// executes, and releases it promptly.
pub struct Ledger {
    conn: Mutex<Connection>,
    /// Maximum entries per (k, ell, n) leaderboard. Configurable at
    /// server start — if reduced, excess entries are trimmed on open.
    pub capacity: u32,
}

impl Ledger {
    /// Open (or create) a SQLite database at the given path and initialize
    /// the schema. Uses the default capacity (10,000).
    pub fn open(path: &str) -> Result<Self, LedgerError> {
        Self::open_with_capacity(path, DEFAULT_LEADERBOARD_CAPACITY)
    }

    /// Open (or create) a SQLite database with a specific leaderboard capacity.
    /// If the database already contains leaderboards exceeding the capacity,
    /// excess entries (lowest ranked) are trimmed on startup.
    pub fn open_with_capacity(path: &str, capacity: u32) -> Result<Self, LedgerError> {
        let conn = Connection::open(path)?;
        schema::init_db(&conn)?;
        enforce_capacity(&conn, capacity)?;
        Ok(Self {
            conn: Mutex::new(conn),
            capacity,
        })
    }

    /// Open an in-memory database (for tests). Uses default capacity (10,000).
    pub fn open_in_memory() -> Result<Self, LedgerError> {
        Self::open_in_memory_with_capacity(DEFAULT_LEADERBOARD_CAPACITY)
    }

    /// Open an in-memory database with a specific leaderboard capacity.
    pub fn open_in_memory_with_capacity(capacity: u32) -> Result<Self, LedgerError> {
        let conn = Connection::open_in_memory()?;
        schema::init_db(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            capacity,
        })
    }
}

/// Trim any leaderboards that exceed the given capacity. Called on startup
/// so the server can shrink capacity without a migration.
fn enforce_capacity(conn: &Connection, capacity: u32) -> Result<(), LedgerError> {
    use rusqlite::params;

    // Find all (k, ell, n) triples that exceed capacity
    let mut stmt = conn.prepare(
        "SELECT k, ell, n, COUNT(*) as cnt FROM leaderboard GROUP BY k, ell, n HAVING cnt > ?1",
    )?;
    let over: Vec<(u32, u32, u32, u32)> = stmt
        .query_map(params![capacity], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (k, ell, n, count) in over {
        let excess = count - capacity;
        tracing::info!(
            k,
            ell,
            n,
            old_count = count,
            new_capacity = capacity,
            evicted = excess,
            "trimming leaderboard to new capacity"
        );
        // Delete the worst entries (highest rank numbers)
        conn.execute(
            "DELETE FROM leaderboard WHERE rowid IN (\
                SELECT rowid FROM leaderboard \
                WHERE k=?1 AND ell=?2 AND n=?3 \
                ORDER BY rank DESC LIMIT ?4\
            )",
            params![k, ell, n, excess],
        )?;
        // Recompute ranks for the trimmed leaderboard
        crate::queries::recompute_ranks(conn, k, ell, n)?;
    }

    Ok(())
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
            goodman_gap: 0,
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
            goodman_gap: 0,
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
    fn capacity_eviction_on_startup() {
        // Create a ledger with capacity 3
        let ledger = Ledger::open_in_memory_with_capacity(3).unwrap();

        // Add 3 entries (fill to capacity)
        for i in 0..3 {
            let cid = format!("cid_{i}");
            ledger
                .store_submission(3, 3, &cid, 5, r#"{"n":5}"#)
                .unwrap();
            let score = AdmitScore {
                tier1_max: 10 - i as u64, // lower = better, so cid_2 is best
                tier1_min: 10 - i as u64,
                goodman_gap: 0,
                tier2_aut: 1.0,
                tier3_cid: cid.clone(),
                score_json: "{}".to_string(),
            };
            ledger.try_admit(3, 3, 5, &cid, &score).unwrap();
        }
        assert_eq!(ledger.get_leaderboard(3, 3, 5).unwrap().len(), 3);

        // Now "reopen" with capacity 2 — should trim the worst entry
        // We simulate this by calling enforce_capacity directly
        {
            let conn = ledger.conn.lock().unwrap();
            enforce_capacity(&conn, 2).unwrap();
        }
        let entries = ledger.get_leaderboard(3, 3, 5).unwrap();
        assert_eq!(entries.len(), 2);
        // The best entries should survive (lowest tier1 scores)
        assert_eq!(entries[0].graph_cid, "cid_2"); // tier1_max=8
        assert_eq!(entries[1].graph_cid, "cid_1"); // tier1_max=9
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
            goodman_gap: 0,
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
