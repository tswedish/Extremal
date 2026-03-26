use anyhow::Result;
use clap::{Parser, Subcommand};
use extremal_identity::{Identity, canonical_payload};
use std::path::PathBuf;

const DEFAULT_CONFIG_DIR: &str = ".config/extremal";
const KEY_FILE: &str = "key.json";

#[derive(Parser)]
#[command(name = "extremal", about = "Extremal CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create the config directory.
    Init,

    /// Generate a new Ed25519 signing keypair.
    Keygen {
        /// Display name for this identity.
        #[arg(long)]
        name: Option<String>,

        /// Output path for the key file. Defaults to .config/extremal/key.json.
        #[arg(long, short)]
        output: Option<String>,
    },

    /// Show current identity.
    Whoami,

    /// Register public key with a server.
    RegisterKey {
        /// Server URL.
        #[arg(long, default_value = "http://localhost:3001")]
        server: String,

        /// GitHub repo link.
        #[arg(long)]
        github_repo: Option<String>,
    },

    /// Score a graph locally (no server needed).
    Score {
        /// Vertex count.
        #[arg(long)]
        n: u32,
        /// graph6 encoding.
        #[arg(long)]
        graph6: String,
        /// Max k for histogram.
        #[arg(long, default_value = "5")]
        max_k: u32,
    },

    /// Submit a graph to a server.
    Submit {
        /// Server URL.
        #[arg(long, default_value = "http://localhost:3001")]
        server: String,
        /// Vertex count.
        #[arg(long)]
        n: u32,
        /// graph6 encoding.
        #[arg(long)]
        graph6: String,
    },

    /// Query a leaderboard.
    Leaderboard {
        /// Server URL.
        #[arg(long, default_value = "http://localhost:3001")]
        server: String,
        /// Vertex count.
        #[arg(long)]
        n: u32,
        /// Maximum entries to show.
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Check server health.
    Health {
        /// Server URL.
        #[arg(long, default_value = "http://localhost:3001")]
        server: String,
    },

    /// Manage workers via the dashboard relay.
    Workers {
        /// Dashboard relay URL.
        #[arg(long, default_value = "http://localhost:4000")]
        relay: String,

        #[command(subcommand)]
        action: WorkerAction,
    },
}

#[derive(Subcommand)]
enum WorkerAction {
    /// List connected workers.
    List,
    /// Get detailed worker status.
    Status {
        /// Worker ID (exact or prefix match).
        worker: String,
    },
    /// Show worker configuration with adjustability info.
    Config {
        /// Worker ID (exact or prefix match).
        worker: String,
    },
    /// Update worker parameters.
    Set {
        /// Worker ID (exact or prefix match).
        worker: String,
        /// Parameters as key=value (e.g. beam_width=200 sample_bias=0.5).
        #[arg(required = true)]
        params: Vec<String>,
    },
    /// Pause worker after current round.
    Pause {
        /// Worker ID (exact or prefix match).
        worker: String,
    },
    /// Resume paused worker.
    Resume {
        /// Worker ID (exact or prefix match).
        worker: String,
    },
    /// Stop worker gracefully.
    Stop {
        /// Worker ID (exact or prefix match).
        worker: String,
    },
}

fn config_dir() -> PathBuf {
    PathBuf::from(DEFAULT_CONFIG_DIR)
}

fn key_path() -> PathBuf {
    config_dir().join(KEY_FILE)
}

fn load_identity() -> Result<Identity> {
    let path = key_path();
    if !path.exists() {
        anyhow::bail!(
            "No key found at {}. Run `extremal keygen` first.",
            path.display()
        );
    }
    Ok(Identity::load(&path)?)
}

// ── HTTP helpers ─────────────────────────────────────────────

async fn api_get(client: &reqwest::Client, url: &str) -> Result<serde_json::Value> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await?;
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(resp.json().await?)
}

async fn api_post(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let resp = client.post(url).json(body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await?;
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(resp.json().await?)
}

async fn api_post_empty(client: &reqwest::Client, url: &str) -> Result<serde_json::Value> {
    let resp = client.post(url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await?;
        anyhow::bail!("request failed ({status}): {text}");
    }
    Ok(resp.json().await?)
}

// ── Main ─────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let dir = config_dir();
            std::fs::create_dir_all(&dir)?;
            println!("Created config directory: {}", dir.display());
        }

        Commands::Keygen { name, output } => {
            let path = if let Some(ref out) = output {
                let p = PathBuf::from(out);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                p
            } else {
                let dir = config_dir();
                std::fs::create_dir_all(&dir)?;
                key_path()
            };
            if path.exists() {
                anyhow::bail!("Key already exists at {}. Delete it first.", path.display());
            }
            let identity = Identity::generate(name);
            identity.save(&path)?;
            println!("Generated keypair:");
            println!("  key_id: {}", identity.key_id);
            if let Some(name) = &identity.display_name {
                println!("  name:   {name}");
            }
            println!("  saved:  {}", path.display());
        }

        Commands::Whoami => {
            let identity = load_identity()?;
            println!("key_id: {}", identity.key_id);
            if let Some(name) = &identity.display_name {
                println!("name:   {name}");
            }
            println!(
                "pubkey: {}",
                hex::encode(identity.verifying_key().as_bytes())
            );
        }

        Commands::RegisterKey {
            server,
            github_repo,
        } => {
            let identity = load_identity()?;
            let pk_hex = hex::encode(identity.verifying_key().as_bytes());

            let mut body = serde_json::json!({ "public_key": pk_hex });
            if let Some(name) = &identity.display_name {
                body["display_name"] = serde_json::json!(name);
            }
            if let Some(repo) = &github_repo {
                body["github_repo"] = serde_json::json!(repo);
            }

            let client = reqwest::Client::new();
            let data = api_post(&client, &format!("{server}/api/keys"), &body).await?;
            println!("Registered with server:");
            println!("  key_id: {}", data["key_id"]);
            println!("  name:   {}", data["display_name"]);
        }

        Commands::Score { n, graph6, max_k } => {
            let matrix = extremal_graph::graph6::decode(&graph6)?;
            if matrix.n() != n {
                anyhow::bail!("graph6 decodes to n={}, expected n={n}", matrix.n());
            }

            let histogram = extremal_scoring::histogram::CliqueHistogram::compute(&matrix, max_k);
            let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
            let gap = extremal_scoring::goodman::goodman_gap(n, red_tri, blue_tri);
            let cid = extremal_graph::compute_cid(&matrix);

            println!("Graph scoring (n={n}):");
            println!("  CID:         {}", cid.to_hex());
            println!("  Histogram:");
            for tier in &histogram.tiers {
                println!(
                    "    k={}: red={}, blue={} -> ({}, {})",
                    tier.k,
                    tier.red,
                    tier.blue,
                    tier.red.max(tier.blue),
                    tier.red.min(tier.blue)
                );
            }
            println!("  Goodman gap: {gap}");
            println!("  Edges:       {}", matrix.num_edges());
        }

        Commands::Submit { server, n, graph6 } => {
            let identity = load_identity()?;
            let payload = canonical_payload(n, &graph6);
            let signature = identity.sign(&payload);

            let body = serde_json::json!({
                "n": n,
                "graph6": graph6,
                "key_id": identity.key_id.as_str(),
                "signature": signature,
            });

            let client = reqwest::Client::new();
            let data = api_post(&client, &format!("{server}/api/submit"), &body).await?;
            println!("Submitted successfully:");
            println!("  CID:      {}", data["cid"]);
            println!("  Verdict:  {}", data["verdict"]);
            println!("  Admitted: {}", data["admitted"]);
            if let Some(rank) = data["rank"].as_i64() {
                println!("  Rank:     {rank}");
            }
        }

        Commands::Leaderboard { server, n, limit } => {
            let client = reqwest::Client::new();
            let data = api_get(
                &client,
                &format!("{server}/api/leaderboards/{n}?limit={limit}"),
            )
            .await?;
            let total = data["total"].as_i64().unwrap_or(0);
            println!("Leaderboard n={n} ({total} entries):");
            if let Some(entries) = data["entries"].as_array() {
                for entry in entries {
                    println!(
                        "  #{}: {} (by {})",
                        entry["rank"], entry["cid"], entry["key_id"]
                    );
                }
            }
        }

        Commands::Health { server } => {
            let client = reqwest::Client::new();
            let data = api_get(&client, &format!("{server}/api/health")).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }

        Commands::Workers { relay, action } => {
            handle_workers_command(&relay, action).await?;
        }
    }

    Ok(())
}

// ── Worker management commands ──────────────────────────────

async fn handle_workers_command(relay: &str, action: WorkerAction) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    match action {
        WorkerAction::List => {
            let data = api_get(&client, &format!("{relay}/api/workers")).await?;

            let workers = data["workers"].as_array();
            let count = data["count"].as_u64().unwrap_or(0);
            println!("Workers ({count} connected via {relay}):");

            if let Some(workers) = workers {
                if workers.is_empty() {
                    println!("  (none)");
                }
                for w in workers {
                    let wid = w["worker_id"].as_str().unwrap_or("?");
                    let kid = w["key_id"].as_str().unwrap_or("?");
                    let n = w["n"].as_u64().unwrap_or(0);
                    let strat = w["strategy"].as_str().unwrap_or("?");
                    let verified = if w["verified"].as_bool().unwrap_or(false) {
                        "verified"
                    } else {
                        "unverified"
                    };
                    let api = w["api_addr"].as_str().unwrap_or("(no API)");
                    println!("  {wid:<16} key={kid:<16} n={n:<4} {strat:<8} {verified:<12} {api}");
                }
            }
        }

        WorkerAction::Status { worker } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;
            let data = api_get(&client, &format!("{api_addr}/api/status")).await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }

        WorkerAction::Config { worker } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;
            let data = api_get(&client, &format!("{api_addr}/api/config")).await?;

            if let Some(params) = data["params"].as_array() {
                println!("Configuration for {worker}:");
                for p in params {
                    let name = p["param"]["name"].as_str().unwrap_or("?");
                    let value = &p["value"];
                    let adjustable = p["param"]["adjustable"].as_bool().unwrap_or(false);
                    let source = p["source"].as_str().unwrap_or("?");
                    let adj_marker = if adjustable { "" } else { " (fixed)" };
                    println!("  {name:<30} = {value:<12} [{source}]{adj_marker}");
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&data)?);
            }
        }

        WorkerAction::Set { worker, params } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;

            // Parse key=value pairs
            let mut patch = serde_json::Map::new();
            for param in &params {
                let (key, val_str) = param.split_once('=').ok_or_else(|| {
                    anyhow::anyhow!("invalid param format: {param} (expected key=value)")
                })?;

                // Try to parse as number first, then bool, then string
                let value: serde_json::Value = if let Ok(v) = val_str.parse::<i64>() {
                    serde_json::json!(v)
                } else if let Ok(v) = val_str.parse::<f64>() {
                    serde_json::json!(v)
                } else if val_str == "true" {
                    serde_json::json!(true)
                } else if val_str == "false" {
                    serde_json::json!(false)
                } else {
                    serde_json::json!(val_str)
                };

                patch.insert(key.to_string(), value);
            }

            let data = api_post(
                &client,
                &format!("{api_addr}/api/config"),
                &serde_json::Value::Object(patch),
            )
            .await?;

            if let Some(applied) = data["applied"].as_array()
                && !applied.is_empty()
            {
                let round = data["effective_round"].as_u64().unwrap_or(0);
                println!("Updated {worker} (effective round {round}):");
                for name in applied {
                    println!("  {}", name.as_str().unwrap_or("?"));
                }
            }
            if let Some(errors) = data["errors"].as_array() {
                for err in errors {
                    if let Some(arr) = err.as_array()
                        && arr.len() == 2
                    {
                        println!(
                            "  error: {} — {}",
                            arr[0].as_str().unwrap_or("?"),
                            arr[1].as_str().unwrap_or("?")
                        );
                    }
                }
            }
        }

        WorkerAction::Pause { worker } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;
            api_post_empty(&client, &format!("{api_addr}/api/pause")).await?;
            println!("Paused {worker}");
        }

        WorkerAction::Resume { worker } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;
            api_post_empty(&client, &format!("{api_addr}/api/resume")).await?;
            println!("Resumed {worker}");
        }

        WorkerAction::Stop { worker } => {
            let api_addr = resolve_worker_api(&client, relay, &worker).await?;
            api_post_empty(&client, &format!("{api_addr}/api/stop")).await?;
            println!("Stopped {worker}");
        }
    }

    Ok(())
}

/// Resolve a worker ID to its API address via the relay server.
async fn resolve_worker_api(
    client: &reqwest::Client,
    relay: &str,
    worker_id: &str,
) -> Result<String> {
    let data = api_get(client, &format!("{relay}/api/workers")).await?;

    let workers = data["workers"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("unexpected response from relay"))?;

    // Exact match first, then prefix match
    let worker = workers
        .iter()
        .find(|w| w["worker_id"].as_str() == Some(worker_id))
        .or_else(|| {
            workers.iter().find(|w| {
                w["worker_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(worker_id))
            })
        })
        .ok_or_else(|| anyhow::anyhow!("worker '{worker_id}' not found on relay"))?;

    let api_addr = worker["api_addr"].as_str().ok_or_else(|| {
        anyhow::anyhow!(
            "worker '{}' has no API endpoint (upgrade worker binary or use --api-port)",
            worker["worker_id"].as_str().unwrap_or(worker_id)
        )
    })?;

    Ok(api_addr.to_string())
}
