//! Worker control protocol: commands, events, and status types.

use serde::{Deserialize, Serialize};

/// Command sent from the control UI to the worker engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerCommand {
    /// Start searching with the given parameters. Only valid in Idle state.
    #[serde(rename = "start")]
    Start {
        k: u32,
        ell: u32,
        n: u32,
        #[serde(default)]
        config: EngineConfigPatch,
    },
    /// Pause the current search. Preserves local pool and state.
    #[serde(rename = "pause")]
    Pause,
    /// Resume a paused search.
    #[serde(rename = "resume")]
    Resume,
    /// Stop the search and return to idle. Clears search state.
    #[serde(rename = "stop")]
    Stop,
    /// Request current status.
    #[serde(rename = "status")]
    Status,
    /// Clear the known CIDs dedup cache.
    #[serde(rename = "clear_known_cids")]
    ClearKnownCids,
    /// Clear the local discovery pool.
    #[serde(rename = "clear_local_pool")]
    ClearLocalPool,
}

/// Partial configuration for starting a search. Missing fields use defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineConfigPatch {
    pub init_mode: Option<String>,
    pub strategy: Option<String>,
    pub max_iters: Option<u64>,
    pub sample_bias: Option<f64>,
    pub noise_flips: Option<u32>,
    pub offline: Option<bool>,
    pub no_backoff: Option<bool>,
    pub server_url: Option<String>,
    /// Strategy-specific config (e.g., {"beam_width": 200, "max_depth": 15})
    #[serde(default)]
    pub strategy_config: Option<serde_json::Value>,
}

/// Event sent from the worker engine to the control UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerEvent {
    /// Current worker status.
    #[serde(rename = "status")]
    Status(WorkerStatus),
    /// Error message.
    #[serde(rename = "error")]
    Error { message: String },
    /// Available strategies with config schemas.
    #[serde(rename = "strategies")]
    Strategies { strategies: Vec<StrategyInfo> },
}

/// Current state of the worker engine.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum WorkerState {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "searching")]
    Searching,
    #[serde(rename = "paused")]
    Paused,
}

/// Status snapshot of the worker, including runtime metrics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub state: WorkerState,
    pub k: Option<u32>,
    pub ell: Option<u32>,
    pub n: Option<u32>,
    pub strategy: Option<String>,
    pub round: u64,
    pub init_mode: Option<String>,
    pub metrics: WorkerMetrics,
}

/// Runtime metrics for the worker engine.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkerMetrics {
    // Buffer sizes
    pub known_cids_count: usize,
    pub local_pool_size: usize,
    pub discovery_buffer_size: usize,

    // Cumulative counters
    pub total_discoveries: u64,
    pub total_submitted: u64,
    pub total_admitted: u64,
    pub total_skipped: u64,

    // Last round timing (ms)
    pub last_round_ms: u64,
    pub last_scoring_ms: u64,
    pub last_submit_ms: u64,

    // Server state
    pub server_connected: bool,
    pub leaderboard_total: u32,
}

/// Description of a registered strategy and its configuration schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub params: Vec<ConfigParam>,
}

/// A configurable parameter exposed by a strategy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigParam {
    pub name: String,
    pub label: String,
    pub description: String,
    pub param_type: ParamType,
    pub default: serde_json::Value,
}

/// Type constraint for a config parameter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ParamType {
    #[serde(rename = "float")]
    Float { min: f64, max: f64 },
    #[serde(rename = "int")]
    Int { min: i64, max: i64 },
    #[serde(rename = "bool")]
    Bool,
}
