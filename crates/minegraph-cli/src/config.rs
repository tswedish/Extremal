//! Project-local config management.
//!
//! Config lives in `.config/minegraph/` relative to the project root
//! (working directory). This keeps config per-worktree.
//!
//! ```
//! .config/minegraph/
//! ├── config.toml     # persistent settings
//! └── key.json        # signing keypair
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default config directory relative to working directory.
const DEFAULT_CONFIG_DIR: &str = ".config/minegraph";

/// Resolve the config directory path.
pub fn resolve_config_dir(override_path: Option<&str>) -> PathBuf {
    match override_path {
        Some(p) => PathBuf::from(p),
        None => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            cwd.join(DEFAULT_CONFIG_DIR)
        }
    }
}

/// Create the config directory if it doesn't exist.
pub fn ensure_config_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create config dir: {}", dir.display()))?;
    }
    Ok(())
}

/// Initialize a fresh config directory with a default config.toml.
pub fn init_config_dir(dir: &Path) -> Result<()> {
    ensure_config_dir(dir)?;
    let config_path = dir.join("config.toml");
    if !config_path.exists() {
        let default = MineGraphConfig::default();
        save_config(&config_path, &default)?;
    }
    Ok(())
}

/// Persistent MineGraph configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MineGraphConfig {
    /// Server URL for submissions and leaderboard sync.
    #[serde(default = "default_server_url")]
    pub server_url: String,

    /// Default search strategy.
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// Default Ramsey parameter k.
    #[serde(default = "default_k")]
    pub k: u32,

    /// Default Ramsey parameter ell.
    #[serde(default = "default_ell")]
    pub ell: u32,

    /// Default target vertex count.
    #[serde(default = "default_n")]
    pub n: u32,

    /// Default beam width.
    #[serde(default = "default_beam_width")]
    pub beam_width: u32,

    /// Default max depth.
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,

    /// Default sample bias for leaderboard seeding.
    #[serde(default = "default_sample_bias")]
    pub sample_bias: f64,

    /// Path to signing key (relative to config dir or absolute).
    #[serde(default)]
    pub signing_key: Option<String>,
}

fn default_server_url() -> String {
    "http://localhost:3001".into()
}
fn default_strategy() -> String {
    "tree2".into()
}
fn default_k() -> u32 {
    5
}
fn default_ell() -> u32 {
    5
}
fn default_n() -> u32 {
    25
}
fn default_beam_width() -> u32 {
    80
}
fn default_max_depth() -> u32 {
    12
}
fn default_sample_bias() -> f64 {
    0.8
}

impl Default for MineGraphConfig {
    fn default() -> Self {
        Self {
            server_url: default_server_url(),
            strategy: default_strategy(),
            k: default_k(),
            ell: default_ell(),
            n: default_n(),
            beam_width: default_beam_width(),
            max_depth: default_max_depth(),
            sample_bias: default_sample_bias(),
            signing_key: None,
        }
    }
}

/// Load config from a TOML file. Returns default if file doesn't exist.
pub fn load_config(path: &Path) -> Result<MineGraphConfig> {
    if !path.exists() {
        return Ok(MineGraphConfig::default());
    }
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let cfg: MineGraphConfig = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config: {}", path.display()))?;
    Ok(cfg)
}

/// Save config to a TOML file.
pub fn save_config(path: &Path, cfg: &MineGraphConfig) -> Result<()> {
    let contents = toml::to_string_pretty(cfg)?;
    fs::write(path, contents)
        .with_context(|| format!("Failed to write config: {}", path.display()))?;
    Ok(())
}

/// Set a config value by key name.
pub fn set_value(cfg: &mut MineGraphConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "server_url" => cfg.server_url = value.to_string(),
        "strategy" => cfg.strategy = value.to_string(),
        "k" => cfg.k = value.parse().context("k must be an integer")?,
        "ell" => cfg.ell = value.parse().context("ell must be an integer")?,
        "n" => cfg.n = value.parse().context("n must be an integer")?,
        "beam_width" => cfg.beam_width = value.parse().context("beam_width must be an integer")?,
        "max_depth" => cfg.max_depth = value.parse().context("max_depth must be an integer")?,
        "sample_bias" => cfg.sample_bias = value.parse().context("sample_bias must be a float")?,
        "signing_key" => cfg.signing_key = Some(value.to_string()),
        _ => anyhow::bail!("Unknown config key: {key}"),
    }
    Ok(())
}

/// Get a config value by key name.
pub fn get_value(cfg: &MineGraphConfig, key: &str) -> Option<String> {
    match key {
        "server_url" => Some(cfg.server_url.clone()),
        "strategy" => Some(cfg.strategy.clone()),
        "k" => Some(cfg.k.to_string()),
        "ell" => Some(cfg.ell.to_string()),
        "n" => Some(cfg.n.to_string()),
        "beam_width" => Some(cfg.beam_width.to_string()),
        "max_depth" => Some(cfg.max_depth.to_string()),
        "sample_bias" => Some(cfg.sample_bias.to_string()),
        "signing_key" => cfg.signing_key.clone(),
        _ => None,
    }
}
