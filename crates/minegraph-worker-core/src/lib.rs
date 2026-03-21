//! Worker engine: leaderboard sync, submission pipeline, init.
//!
//! The worker engine runs search rounds in a loop:
//! 1. Sync known CIDs from server
//! 2. Fetch seed graph from leaderboard
//! 3. Run strategy search
//! 4. Score and submit discoveries

pub mod client;
pub mod dashboard;
pub mod engine;
