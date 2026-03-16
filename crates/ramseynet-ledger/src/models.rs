use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A graph submission indexed by (k, ell, n).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submission {
    pub graph_cid: String,
    pub k: u32,
    pub ell: u32,
    pub n: u32,
    pub rgxf_json: String,
    pub submitted_at: DateTime<Utc>,
    /// Signing key ID (if signed).
    pub key_id: Option<String>,
    /// Signature verification status: "anonymous", "verified", "invalid", "unregistered".
    pub sig_status: String,
    /// Arbitrary JSON metadata from the submitter (commit hash, worker ID, etc.).
    pub metadata: Option<String>,
}

/// A verification receipt for a submitted graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub receipt_id: i64,
    pub graph_cid: String,
    pub k: u32,
    pub ell: u32,
    pub verdict: String,
    pub reason: Option<String>,
    pub witness: Option<Vec<u32>>,
    pub verified_at: DateTime<Utc>,
}

/// A ranked entry on a (k, ell, n) leaderboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub k: u32,
    pub ell: u32,
    pub n: u32,
    pub graph_cid: String,
    pub rank: u32,
    pub tier1_max: u64,
    pub tier1_min: u64,
    pub goodman_gap: u64,
    pub tier2_aut: f64,
    pub score_json: String,
    pub key_id: Option<String>,
    pub metadata: Option<String>,
    pub admitted_at: DateTime<Utc>,
}

/// A registered identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub key_id: String,
    pub public_key: String,
    pub display_name: Option<String>,
    pub github_repo: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A paginated slice of a leaderboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderboardPage {
    pub entries: Vec<LeaderboardEntry>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

/// Summary of a (k, ell, n) leaderboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaderboardSummary {
    pub k: u32,
    pub ell: u32,
    pub n: u32,
    pub entry_count: u32,
    pub top_cid: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}
