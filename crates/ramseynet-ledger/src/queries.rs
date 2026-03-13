use chrono::Utc;
use rusqlite::params;

use crate::error::LedgerError;
use crate::models::*;
use crate::Ledger;

/// Maximum entries per (k, ell, n) leaderboard.
const LEADERBOARD_CAPACITY: u32 = 100;

// ── Submission + Receipt operations ──────────────────────────────────

impl Ledger {
    /// Store a graph submission. Enforces k <= ell canonical form.
    /// Returns `GraphAlreadySubmitted` if the CID exists.
    pub fn store_submission(
        &self,
        k: u32,
        ell: u32,
        graph_cid: &str,
        n: u32,
        rgxf_json: &str,
    ) -> Result<Submission, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let now = Utc::now();
        let conn = self.conn.lock().unwrap();
        let result = conn.execute(
            "INSERT INTO graph_submissions (graph_cid, k, ell, n, rgxf_json, submitted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![graph_cid, k, ell, n, rgxf_json, now.to_rfc3339()],
        );
        match result {
            Ok(_) => Ok(Submission {
                graph_cid: graph_cid.to_string(),
                k,
                ell,
                n,
                rgxf_json: rgxf_json.to_string(),
                submitted_at: now,
            }),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(LedgerError::GraphAlreadySubmitted(graph_cid.to_string()))
            }
            Err(e) => Err(LedgerError::Db(e)),
        }
    }

    /// Store a verification receipt.
    pub fn store_receipt(
        &self,
        graph_cid: &str,
        k: u32,
        ell: u32,
        verdict: &str,
        reason: Option<&str>,
        witness: Option<&[u32]>,
    ) -> Result<Receipt, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let now = Utc::now();
        let witness_json = witness.map(|w| serde_json::to_string(w).unwrap());
        let conn = self.conn.lock().unwrap();
        let receipt_id = conn.query_row(
            "INSERT INTO verify_receipts (graph_cid, k, ell, verdict, reason, witness_json, verified_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING receipt_id",
            params![graph_cid, k, ell, verdict, reason, witness_json, now.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(Receipt {
            receipt_id,
            graph_cid: graph_cid.to_string(),
            k,
            ell,
            verdict: verdict.to_string(),
            reason: reason.map(|s| s.to_string()),
            witness: witness.map(|w| w.to_vec()),
            verified_at: now,
        })
    }
}

// ── Leaderboard operations ──────────────────────────────────────────

/// Score components used for leaderboard admission comparison.
#[derive(Debug, Clone)]
pub struct AdmitScore {
    pub tier1_max: u64,
    pub tier1_min: u64,
    pub tier2_aut: f64,
    pub tier3_cid: String,
    pub score_json: String,
}

/// Info about the admission threshold for a leaderboard.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThresholdInfo {
    pub entry_count: u32,
    pub capacity: u32,
    /// Worst entry's score components (None if board not full).
    pub worst_tier1_max: Option<u64>,
    pub worst_tier1_min: Option<u64>,
    pub worst_tier2_aut: Option<f64>,
    pub worst_tier3_cid: Option<String>,
}

impl Ledger {
    /// Try to admit a graph to the (k, ell, n) leaderboard.
    /// Returns the entry if admitted, None if rejected.
    pub fn try_admit(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        graph_cid: &str,
        score: &AdmitScore,
    ) -> Result<Option<LeaderboardEntry>, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let conn = self.conn.lock().unwrap();

        // Check for duplicate
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 AND graph_cid=?4",
                params![k, ell, n, graph_cid],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            // Already on the board — return existing entry
            let entry = conn.query_row(
                "SELECT k, ell, n, graph_cid, rank, tier1_max, tier1_min, tier2_aut, score_json, admitted_at FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 AND graph_cid=?4",
                params![k, ell, n, graph_cid],
                |row| {
                    Ok(LeaderboardEntry {
                        k: row.get(0)?,
                        ell: row.get(1)?,
                        n: row.get(2)?,
                        graph_cid: row.get(3)?,
                        rank: row.get(4)?,
                        tier1_max: row.get::<_, i64>(5)? as u64,
                        tier1_min: row.get::<_, i64>(6)? as u64,
                        tier2_aut: row.get(7)?,
                        score_json: row.get(8)?,
                        admitted_at: parse_datetime(row.get::<_, String>(9)?),
                    })
                },
            )?;
            return Ok(Some(entry));
        }

        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3",
            params![k, ell, n],
            |row| row.get(0),
        )?;

        // If full, check against worst entry
        if count >= LEADERBOARD_CAPACITY {
            // Get the worst entry (highest rank number)
            let worst = conn.query_row(
                "SELECT tier1_max, tier1_min, tier2_aut, tier3_cid, graph_cid FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 ORDER BY rank DESC LIMIT 1",
                params![k, ell, n],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, f64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )?;

            // Compare: new entry must be strictly better than worst
            let is_better = score_cmp(
                score.tier1_max,
                score.tier1_min,
                score.tier2_aut,
                &score.tier3_cid,
                worst.0,
                worst.1,
                worst.2,
                &worst.3,
            ) == std::cmp::Ordering::Less;

            if !is_better {
                return Ok(None);
            }

            // Delete the worst entry
            conn.execute(
                "DELETE FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 AND graph_cid=?4",
                params![k, ell, n, worst.4],
            )?;
        }

        // Insert the new entry (with temporary rank=100, will be recomputed)
        let now = Utc::now();
        conn.execute(
            "INSERT INTO leaderboard (k, ell, n, graph_cid, rank, tier1_max, tier1_min, tier2_aut, tier3_cid, score_json, admitted_at) VALUES (?1, ?2, ?3, ?4, 100, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                k, ell, n, graph_cid,
                score.tier1_max as i64,
                score.tier1_min as i64,
                score.tier2_aut,
                score.tier3_cid,
                score.score_json,
                now.to_rfc3339()
            ],
        )?;

        // Recompute ranks for this (k, ell, n) leaderboard
        recompute_ranks(&conn, k, ell, n)?;

        // Return the admitted entry
        let entry = conn.query_row(
            "SELECT k, ell, n, graph_cid, rank, tier1_max, tier1_min, tier2_aut, score_json, admitted_at FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 AND graph_cid=?4",
            params![k, ell, n, graph_cid],
            |row| {
                Ok(LeaderboardEntry {
                    k: row.get(0)?,
                    ell: row.get(1)?,
                    n: row.get(2)?,
                    graph_cid: row.get(3)?,
                    rank: row.get(4)?,
                    tier1_max: row.get::<_, i64>(5)? as u64,
                    tier1_min: row.get::<_, i64>(6)? as u64,
                    tier2_aut: row.get(7)?,
                    score_json: row.get(8)?,
                    admitted_at: parse_datetime(row.get::<_, String>(9)?),
                })
            },
        )?;

        Ok(Some(entry))
    }

    /// Get the admission threshold for a leaderboard.
    pub fn get_threshold(&self, k: u32, ell: u32, n: u32) -> Result<ThresholdInfo, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let conn = self.conn.lock().unwrap();

        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3",
            params![k, ell, n],
            |row| row.get(0),
        )?;

        if count < LEADERBOARD_CAPACITY {
            return Ok(ThresholdInfo {
                entry_count: count,
                capacity: LEADERBOARD_CAPACITY,
                worst_tier1_max: None,
                worst_tier1_min: None,
                worst_tier2_aut: None,
                worst_tier3_cid: None,
            });
        }

        // Board is full — return the worst entry's score
        let worst = conn.query_row(
            "SELECT tier1_max, tier1_min, tier2_aut, tier3_cid FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 ORDER BY rank DESC LIMIT 1",
            params![k, ell, n],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, f64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;

        Ok(ThresholdInfo {
            entry_count: count,
            capacity: LEADERBOARD_CAPACITY,
            worst_tier1_max: Some(worst.0),
            worst_tier1_min: Some(worst.1),
            worst_tier2_aut: Some(worst.2),
            worst_tier3_cid: Some(worst.3),
        })
    }

    /// Get the full leaderboard for a (k, ell, n) triple.
    pub fn get_leaderboard(
        &self,
        k: u32,
        ell: u32,
        n: u32,
    ) -> Result<Vec<LeaderboardEntry>, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT k, ell, n, graph_cid, rank, tier1_max, tier1_min, tier2_aut, score_json, admitted_at FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3 ORDER BY rank",
        )?;
        let rows = stmt.query_map(params![k, ell, n], |row| {
            Ok(LeaderboardEntry {
                k: row.get(0)?,
                ell: row.get(1)?,
                n: row.get(2)?,
                graph_cid: row.get(3)?,
                rank: row.get(4)?,
                tier1_max: row.get::<_, i64>(5)? as u64,
                tier1_min: row.get::<_, i64>(6)? as u64,
                tier2_aut: row.get(7)?,
                score_json: row.get(8)?,
                admitted_at: parse_datetime(row.get::<_, String>(9)?),
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// List all distinct (k, ell, n) leaderboards with summary info.
    pub fn list_leaderboards(&self) -> Result<Vec<LeaderboardSummary>, LedgerError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT k, ell, n, COUNT(*) as cnt, \
             (SELECT graph_cid FROM leaderboard lb2 WHERE lb2.k=lb.k AND lb2.ell=lb.ell AND lb2.n=lb.n AND lb2.rank=1) as top_cid, \
             MAX(admitted_at) as last_updated \
             FROM leaderboard lb \
             GROUP BY k, ell, n \
             ORDER BY k, ell, n",
        )?;
        let rows = stmt.query_map([], |row| {
            let last_str: Option<String> = row.get(5)?;
            Ok(LeaderboardSummary {
                k: row.get(0)?,
                ell: row.get(1)?,
                n: row.get(2)?,
                entry_count: row.get(3)?,
                top_cid: row.get(4)?,
                last_updated: last_str.map(parse_datetime),
            })
        })?;
        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(row?);
        }
        Ok(summaries)
    }

    /// List available n values for a given (k, ell) pair.
    pub fn list_n_for_pair(&self, k: u32, ell: u32) -> Result<Vec<u32>, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT n FROM leaderboard WHERE k=?1 AND ell=?2 ORDER BY n",
        )?;
        let rows = stmt.query_map(params![k, ell], |row| row.get(0))?;
        let mut ns = Vec::new();
        for row in rows {
            ns.push(row?);
        }
        Ok(ns)
    }

    /// Get RGXF JSON for the top `limit` leaderboard entries for a (k, ell, n) triple.
    pub fn get_leaderboard_graphs(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        limit: u32,
    ) -> Result<Vec<String>, LedgerError> {
        let (k, ell) = canonical(k, ell);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT gs.rgxf_json FROM leaderboard lb \
             JOIN graph_submissions gs ON gs.graph_cid = lb.graph_cid \
             WHERE lb.k=?1 AND lb.ell=?2 AND lb.n=?3 \
             ORDER BY lb.rank LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![k, ell, n, limit], |row| row.get(0))?;
        let mut rgxfs = Vec::new();
        for row in rows {
            rgxfs.push(row?);
        }
        Ok(rgxfs)
    }

    /// Get the RGXF JSON string for a submission by CID.
    pub fn get_submission_rgxf(&self, cid: &str) -> Result<Option<String>, LedgerError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT rgxf_json FROM graph_submissions WHERE graph_cid = ?1",
            params![cid],
            |row| row.get(0),
        )
        .optional()
        .map_err(LedgerError::Db)
    }

    /// Get full submission detail by CID: submission + optional receipt + optional leaderboard entry.
    #[allow(clippy::type_complexity)]
    pub fn get_submission_detail(
        &self,
        cid: &str,
    ) -> Result<Option<(Submission, Option<Receipt>, Option<LeaderboardEntry>)>, LedgerError> {
        let conn = self.conn.lock().unwrap();

        // 1. Get submission
        let submission: Option<Submission> = conn
            .query_row(
                "SELECT graph_cid, k, ell, n, rgxf_json, submitted_at FROM graph_submissions WHERE graph_cid = ?1",
                params![cid],
                |row| {
                    Ok(Submission {
                        graph_cid: row.get(0)?,
                        k: row.get(1)?,
                        ell: row.get(2)?,
                        n: row.get(3)?,
                        rgxf_json: row.get(4)?,
                        submitted_at: parse_datetime(row.get::<_, String>(5)?),
                    })
                },
            )
            .optional()
            .map_err(LedgerError::Db)?;

        let submission = match submission {
            Some(s) => s,
            None => return Ok(None),
        };

        // 2. Get receipt (optional — may not be verified yet)
        let receipt: Option<Receipt> = conn
            .query_row(
                "SELECT receipt_id, graph_cid, k, ell, verdict, reason, witness_json, verified_at FROM verify_receipts WHERE graph_cid = ?1",
                params![cid],
                |row| {
                    let witness_str: Option<String> = row.get(6)?;
                    let witness: Option<Vec<u32>> =
                        witness_str.and_then(|s| serde_json::from_str(&s).ok());
                    Ok(Receipt {
                        receipt_id: row.get(0)?,
                        graph_cid: row.get(1)?,
                        k: row.get(2)?,
                        ell: row.get(3)?,
                        verdict: row.get(4)?,
                        reason: row.get(5)?,
                        witness,
                        verified_at: parse_datetime(row.get::<_, String>(7)?),
                    })
                },
            )
            .optional()
            .map_err(LedgerError::Db)?;

        // 3. Get leaderboard entry (optional — may not be admitted)
        let lb_entry: Option<LeaderboardEntry> = conn
            .query_row(
                "SELECT k, ell, n, graph_cid, rank, tier1_max, tier1_min, tier2_aut, score_json, admitted_at FROM leaderboard WHERE graph_cid = ?1 LIMIT 1",
                params![cid],
                |row| {
                    Ok(LeaderboardEntry {
                        k: row.get(0)?,
                        ell: row.get(1)?,
                        n: row.get(2)?,
                        graph_cid: row.get(3)?,
                        rank: row.get(4)?,
                        tier1_max: row.get::<_, i64>(5)? as u64,
                        tier1_min: row.get::<_, i64>(6)? as u64,
                        tier2_aut: row.get(7)?,
                        score_json: row.get(8)?,
                        admitted_at: parse_datetime(row.get::<_, String>(9)?),
                    })
                },
            )
            .optional()
            .map_err(LedgerError::Db)?;

        Ok(Some((submission, receipt, lb_entry)))
    }
}
// ── Helpers ──────────────────────────────────────────────────────────

/// Enforce k <= ell canonical form.
fn canonical(k: u32, ell: u32) -> (u32, u32) {
    if k <= ell {
        (k, ell)
    } else {
        (ell, k)
    }
}

/// Recompute ranks for a (k, ell, n) leaderboard by fetching all entries,
/// sorting in Rust, and writing ranks back. Max 100 rows — trivially fast.
fn recompute_ranks(
    conn: &rusqlite::Connection,
    k: u32,
    ell: u32,
    n: u32,
) -> Result<(), LedgerError> {
    let mut stmt = conn.prepare(
        "SELECT graph_cid, tier1_max, tier1_min, tier2_aut, tier3_cid FROM leaderboard WHERE k=?1 AND ell=?2 AND n=?3",
    )?;
    let mut entries: Vec<(String, u64, u64, f64, String)> = stmt
        .query_map(params![k, ell, n], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    entries.sort_by(|a, b| score_cmp(a.1, a.2, a.3, &a.4, b.1, b.2, b.3, &b.4));

    for (rank, entry) in entries.iter().enumerate() {
        conn.execute(
            "UPDATE leaderboard SET rank=?1 WHERE k=?2 AND ell=?3 AND n=?4 AND graph_cid=?5",
            params![rank as u32 + 1, k, ell, n, entry.0],
        )?;
    }
    Ok(())
}

/// Compare two score tuples using the 3-tier ordering.
#[allow(clippy::too_many_arguments)]
fn score_cmp(
    a_t1_max: u64,
    a_t1_min: u64,
    a_t2_aut: f64,
    a_t3_cid: &str,
    b_t1_max: u64,
    b_t1_min: u64,
    b_t2_aut: f64,
    b_t3_cid: &str,
) -> std::cmp::Ordering {
    // T1: lower clique counts win (ascending)
    (a_t1_max, a_t1_min)
        .cmp(&(b_t1_max, b_t1_min))
        // T2: higher aut wins (descending)
        .then(b_t2_aut.total_cmp(&a_t2_aut))
        // T3: smaller CID wins (ascending)
        .then(a_t3_cid.cmp(b_t3_cid))
}

/// Helper trait to convert `QueryReturnedNoRows` into `None`.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Parse an ISO 8601 datetime string. Falls back to epoch on parse error.
fn parse_datetime(s: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}
