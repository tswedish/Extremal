use rusqlite::Connection;

use crate::LedgerError;

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('version', '5');

CREATE TABLE IF NOT EXISTS graph_submissions (
    graph_cid    TEXT PRIMARY KEY,
    k            INTEGER NOT NULL,
    ell          INTEGER NOT NULL,
    n            INTEGER NOT NULL,
    rgxf_json    TEXT NOT NULL,
    key_id       TEXT,
    submitted_at TEXT NOT NULL,
    CHECK (k >= 2 AND ell >= 2 AND k <= ell AND n >= 1)
);

CREATE TABLE IF NOT EXISTS verify_receipts (
    receipt_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    graph_cid    TEXT NOT NULL UNIQUE REFERENCES graph_submissions(graph_cid),
    k            INTEGER NOT NULL,
    ell          INTEGER NOT NULL,
    verdict      TEXT NOT NULL,
    reason       TEXT,
    witness_json TEXT,
    verified_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS leaderboard (
    k            INTEGER NOT NULL,
    ell          INTEGER NOT NULL,
    n            INTEGER NOT NULL,
    graph_cid    TEXT NOT NULL REFERENCES graph_submissions(graph_cid),
    rank         INTEGER NOT NULL,
    tier1_max    INTEGER NOT NULL,
    tier1_min    INTEGER NOT NULL,
    goodman_gap  INTEGER NOT NULL DEFAULT 0,
    tier2_aut    REAL NOT NULL,
    tier3_cid    TEXT NOT NULL,
    score_json   TEXT NOT NULL,
    key_id       TEXT,
    admitted_at  TEXT NOT NULL,
    PRIMARY KEY (k, ell, n, graph_cid),
    CHECK (k <= ell AND rank >= 1)
);

";

// Migrations for existing databases — add key_id columns if missing.
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE graph_submissions ADD COLUMN key_id TEXT;",
    "ALTER TABLE leaderboard ADD COLUMN key_id TEXT;",
];

/// Initialize the database schema. Enables WAL mode for better concurrent reads.
/// Applies migrations for existing databases (adding key_id columns).
pub fn init_db(conn: &Connection) -> Result<(), LedgerError> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(SCHEMA_SQL)?;

    // Apply migrations silently (columns may already exist on fresh DBs)
    for migration in MIGRATIONS {
        let _ = conn.execute_batch(migration); // ignore "duplicate column" errors
    }

    Ok(())
}
