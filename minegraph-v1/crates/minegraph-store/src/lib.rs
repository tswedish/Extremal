//! PostgreSQL data layer for MineGraph.
//!
//! Provides the [`Store`] type wrapping a `sqlx::PgPool` with repository
//! methods for graphs, submissions, leaderboards, and identities.
//!
//! All queries use runtime SQL (not compile-time checked) so the crate
//! builds without a running database.

pub mod models;

use chrono::Utc;
use models::*;
use sqlx::PgPool;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("not found: {0}")]
    NotFound(String),
}

/// The main data store, wrapping a PostgreSQL connection pool.
#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Create a new store from a connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the underlying pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run database migrations.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        Ok(())
    }

    /// Health check: verify the database is reachable.
    pub async fn health_check(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }

    // ── Identity operations ─────────────────────────────────────

    /// Register a new identity (public key).
    pub async fn register_identity(
        &self,
        key_id: &str,
        public_key: &str,
        display_name: Option<&str>,
        github_repo: Option<&str>,
    ) -> Result<Identity, StoreError> {
        let row = sqlx::query_as::<_, Identity>(
            "INSERT INTO identities (key_id, public_key, display_name, github_repo)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (key_id) DO UPDATE SET
                display_name = COALESCE(EXCLUDED.display_name, identities.display_name),
                github_repo = COALESCE(EXCLUDED.github_repo, identities.github_repo)
             RETURNING *",
        )
        .bind(key_id)
        .bind(public_key)
        .bind(display_name)
        .bind(github_repo)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Look up an identity by key_id.
    pub async fn get_identity(&self, key_id: &str) -> Result<Option<Identity>, StoreError> {
        let row = sqlx::query_as::<_, Identity>("SELECT * FROM identities WHERE key_id = $1")
            .bind(key_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // ── Graph operations ────────────────────────────────────────

    /// Store a graph (upsert by CID).
    pub async fn store_graph(&self, cid: &str, n: i32, graph6: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO graphs (cid, n, graph6)
             VALUES ($1, $2, $3)
             ON CONFLICT (cid) DO NOTHING",
        )
        .bind(cid)
        .bind(n)
        .bind(graph6)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a graph by CID.
    pub async fn get_graph(&self, cid: &str) -> Result<Option<Graph>, StoreError> {
        let row = sqlx::query_as::<_, Graph>("SELECT * FROM graphs WHERE cid = $1")
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // ── Score operations ────────────────────────────────────────

    /// Store a precomputed score for a graph.
    pub async fn store_score(
        &self,
        cid: &str,
        n: i32,
        histogram: &serde_json::Value,
        goodman_gap: f64,
        aut_order: f64,
        score_bytes: &[u8],
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO scores (cid, n, histogram, goodman_gap, aut_order, score_bytes)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (cid) DO NOTHING",
        )
        .bind(cid)
        .bind(n)
        .bind(histogram)
        .bind(goodman_gap)
        .bind(aut_order)
        .bind(score_bytes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get the score for a graph.
    pub async fn get_score(&self, cid: &str) -> Result<Option<Score>, StoreError> {
        let row = sqlx::query_as::<_, Score>("SELECT * FROM scores WHERE cid = $1")
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // ── Submission operations ───────────────────────────────────

    /// Record a submission.
    pub async fn store_submission(
        &self,
        cid: &str,
        key_id: &str,
        signature: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<Submission, StoreError> {
        let row = sqlx::query_as::<_, Submission>(
            "INSERT INTO submissions (cid, key_id, signature, metadata)
             VALUES ($1, $2, $3, $4)
             RETURNING *",
        )
        .bind(cid)
        .bind(key_id)
        .bind(signature)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Get a submission by CID (most recent).
    pub async fn get_submission(&self, cid: &str) -> Result<Option<Submission>, StoreError> {
        let row = sqlx::query_as::<_, Submission>(
            "SELECT * FROM submissions WHERE cid = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(cid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Get submissions by identity.
    pub async fn get_submissions_by_identity(
        &self,
        key_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Submission>, StoreError> {
        let rows = sqlx::query_as::<_, Submission>(
            "SELECT * FROM submissions WHERE key_id = $1
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(key_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── Leaderboard operations ──────────────────────────────────

    /// Get the leaderboard for a given n (paginated).
    pub async fn get_leaderboard(
        &self,
        n: i32,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardEntry>, StoreError> {
        let rows = sqlx::query_as::<_, LeaderboardEntry>(
            "SELECT * FROM leaderboard WHERE n = $1
             ORDER BY score_bytes ASC LIMIT $2 OFFSET $3",
        )
        .bind(n)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get all distinct n values that have leaderboard entries.
    pub async fn list_leaderboard_ns(&self) -> Result<Vec<LeaderboardSummary>, StoreError> {
        let rows = sqlx::query_as::<_, LeaderboardSummary>(
            "SELECT n, COUNT(*) as entry_count,
                    MIN(score_bytes) as best_score_bytes
             FROM leaderboard GROUP BY n ORDER BY n",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get the entry count for a leaderboard.
    pub async fn leaderboard_count(&self, n: i32) -> Result<i64, StoreError> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM leaderboard WHERE n = $1")
            .bind(n)
            .fetch_one(&self.pool)
            .await?;
        Ok(count.0)
    }

    /// Get the worst (highest rank) entry's score for admission threshold.
    pub async fn leaderboard_threshold(&self, n: i32) -> Result<Option<Vec<u8>>, StoreError> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT score_bytes FROM leaderboard WHERE n = $1
             ORDER BY rank DESC LIMIT 1",
        )
        .bind(n)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    /// Try to admit a graph to the leaderboard. Returns the new rank if admitted,
    /// or None if the graph didn't make the cut.
    ///
    /// Lightweight admission: insert + count better entries for rank.
    /// No full-table rerank — O(1) per admission instead of O(capacity).
    pub async fn try_admit(
        &self,
        n: i32,
        cid: &str,
        key_id: &str,
        score_bytes: &[u8],
        capacity: i32,
    ) -> Result<Option<i32>, StoreError> {
        let mut tx = self.pool.begin().await?;

        // Check if already on this leaderboard
        let existing: Option<(i32,)> =
            sqlx::query_as("SELECT rank FROM leaderboard WHERE n = $1 AND cid = $2")
                .bind(n)
                .bind(cid)
                .fetch_optional(&mut *tx)
                .await?;

        if existing.is_some() {
            tx.commit().await?;
            return Ok(None);
        }

        // Count current entries
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM leaderboard WHERE n = $1")
            .bind(n)
            .fetch_one(&mut *tx)
            .await?;

        if count >= capacity as i64 {
            // Check if we beat the worst entry
            let worst: Option<(Vec<u8>, String)> = sqlx::query_as(
                "SELECT score_bytes, cid FROM leaderboard WHERE n = $1
                 ORDER BY score_bytes DESC LIMIT 1",
            )
            .bind(n)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some((worst_score, worst_cid)) = worst {
                if score_bytes >= worst_score.as_slice() {
                    tx.commit().await?;
                    return Ok(None);
                }
                // Evict the worst
                sqlx::query("DELETE FROM leaderboard WHERE n = $1 AND cid = $2")
                    .bind(n)
                    .bind(&worst_cid)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        // Compute rank: count how many entries have a strictly better (lower) score
        let (better_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM leaderboard WHERE n = $1 AND score_bytes < $2")
                .bind(n)
                .bind(score_bytes)
                .fetch_one(&mut *tx)
                .await?;
        let new_rank = (better_count + 1) as i32;

        // Insert with computed rank
        sqlx::query(
            "INSERT INTO leaderboard (n, cid, key_id, score_bytes, rank)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (n, cid) DO NOTHING",
        )
        .bind(n)
        .bind(cid)
        .bind(key_id)
        .bind(score_bytes)
        .bind(new_rank)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(new_rank))
    }

    /// Get CIDs on the leaderboard for incremental sync.
    pub async fn get_leaderboard_cids(
        &self,
        n: i32,
        since: Option<chrono::DateTime<Utc>>,
    ) -> Result<Vec<String>, StoreError> {
        let rows: Vec<(String,)> = if let Some(since) = since {
            sqlx::query_as(
                "SELECT cid FROM leaderboard WHERE n = $1 AND admitted_at > $2
                 ORDER BY admitted_at",
            )
            .bind(n)
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as("SELECT cid FROM leaderboard WHERE n = $1 ORDER BY score_bytes")
                .bind(n)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Get graph6 data for leaderboard entries (for worker seeding).
    pub async fn get_leaderboard_graphs(
        &self,
        n: i32,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LeaderboardGraphRow>, StoreError> {
        let rows = sqlx::query_as::<_, LeaderboardGraphRow>(
            "SELECT l.rank, l.cid, l.score_bytes, g.graph6
             FROM leaderboard l
             JOIN graphs g ON l.cid = g.cid
             WHERE l.n = $1
             ORDER BY l.score_bytes ASC
             LIMIT $2 OFFSET $3",
        )
        .bind(n)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── Receipt operations ──────────────────────────────────────

    /// Store a server-signed verification receipt.
    pub async fn store_receipt(
        &self,
        cid: &str,
        server_key_id: &str,
        verdict: &str,
        score_json: Option<&serde_json::Value>,
        signature: &str,
    ) -> Result<Receipt, StoreError> {
        let row = sqlx::query_as::<_, Receipt>(
            "INSERT INTO receipts (cid, server_key_id, verdict, score_json, signature)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING *",
        )
        .bind(cid)
        .bind(server_key_id)
        .bind(verdict)
        .bind(score_json)
        .bind(signature)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Get the latest receipt for a graph.
    pub async fn get_receipt(&self, cid: &str) -> Result<Option<Receipt>, StoreError> {
        let row = sqlx::query_as::<_, Receipt>(
            "SELECT * FROM receipts WHERE cid = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(cid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // ── Server config ───────────────────────────────────────────

    /// Get a server config value.
    pub async fn get_config(&self, key: &str) -> Result<Option<serde_json::Value>, StoreError> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT value FROM server_config WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    /// Set a server config value.
    pub async fn set_config(&self, key: &str, value: &serde_json::Value) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO server_config (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
