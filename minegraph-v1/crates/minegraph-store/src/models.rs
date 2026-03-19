//! Database model types for MineGraph.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A registered identity (worker or participant).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Identity {
    pub key_id: String,
    pub public_key: String,
    pub display_name: Option<String>,
    pub github_repo: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A stored graph (deduplicated by CID).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Graph {
    pub cid: String,
    pub n: i32,
    pub graph6: String,
    pub created_at: DateTime<Utc>,
}

/// A graph submission.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Submission {
    pub id: i64,
    pub cid: String,
    pub key_id: String,
    pub signature: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// A precomputed score for a graph.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Score {
    pub cid: String,
    pub n: i32,
    pub histogram: serde_json::Value,
    pub goodman_gap: f64,
    pub aut_order: f64,
    pub score_bytes: Vec<u8>,
    pub computed_at: DateTime<Utc>,
}

/// A leaderboard entry.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardEntry {
    pub n: i32,
    pub rank: i32,
    pub cid: String,
    pub key_id: String,
    pub score_bytes: Vec<u8>,
    pub admitted_at: DateTime<Utc>,
}

/// Summary of a leaderboard (for listing all n values).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardSummary {
    pub n: i32,
    pub entry_count: i64,
}

/// A leaderboard entry joined with graph6 data.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardGraphRow {
    pub rank: i32,
    pub cid: String,
    pub score_bytes: Vec<u8>,
    pub graph6: String,
}

/// A rich leaderboard entry with graph data and scores.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeaderboardRichEntry {
    pub rank: i32,
    pub cid: String,
    pub key_id: String,
    pub graph6: String,
    pub goodman_gap: Option<f64>,
    pub aut_order: Option<f64>,
    pub histogram: Option<serde_json::Value>,
    pub admitted_at: DateTime<Utc>,
}

/// A verification receipt signed by the server.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Receipt {
    pub id: i64,
    pub cid: String,
    pub server_key_id: String,
    pub verdict: String,
    pub score_json: Option<serde_json::Value>,
    pub signature: String,
    pub created_at: DateTime<Utc>,
}
