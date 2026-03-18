use anyhow::Result;
use clap::{Parser, Subcommand};
use minegraph_identity::{Identity, canonical_payload};
use std::path::PathBuf;

const DEFAULT_CONFIG_DIR: &str = ".config/minegraph";
const KEY_FILE: &str = "key.json";

#[derive(Parser)]
#[command(name = "minegraph", about = "MineGraph CLI tool")]
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
            "No key found at {}. Run `minegraph keygen` first.",
            path.display()
        );
    }
    Ok(Identity::load(&path)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let dir = config_dir();
            std::fs::create_dir_all(&dir)?;
            println!("Created config directory: {}", dir.display());
        }

        Commands::Keygen { name } => {
            let dir = config_dir();
            std::fs::create_dir_all(&dir)?;
            let path = key_path();
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

            let mut body = serde_json::json!({
                "public_key": pk_hex,
            });
            if let Some(name) = &identity.display_name {
                body["display_name"] = serde_json::json!(name);
            }
            if let Some(repo) = &github_repo {
                body["github_repo"] = serde_json::json!(repo);
            }

            let client = reqwest::Client::new();
            let resp = client
                .post(format!("{server}/api/keys"))
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await?;
                println!("Registered with server:");
                println!("  key_id: {}", data["key_id"]);
                println!("  name:   {}", data["display_name"]);
            } else {
                let status = resp.status();
                let text = resp.text().await?;
                anyhow::bail!("Registration failed ({status}): {text}");
            }
        }

        Commands::Score { n, graph6, max_k } => {
            let matrix = minegraph_graph::graph6::decode(&graph6)?;
            if matrix.n() != n {
                anyhow::bail!("graph6 decodes to n={}, expected n={n}", matrix.n());
            }

            let histogram = minegraph_scoring::histogram::CliqueHistogram::compute(&matrix, max_k);
            let (red_tri, blue_tri) = histogram.tier(3).map(|t| (t.red, t.blue)).unwrap_or((0, 0));
            let gap = minegraph_scoring::goodman::goodman_gap(n, red_tri, blue_tri);
            let cid = minegraph_graph::compute_cid(&matrix);

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
            let resp = client
                .post(format!("{server}/api/submit"))
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await?;
                println!("Submitted successfully:");
                println!("  CID:      {}", data["cid"]);
                println!("  Verdict:  {}", data["verdict"]);
                println!("  Admitted: {}", data["admitted"]);
                if let Some(rank) = data["rank"].as_i64() {
                    println!("  Rank:     {rank}");
                }
            } else {
                let status = resp.status();
                let text = resp.text().await?;
                anyhow::bail!("Submission failed ({status}): {text}");
            }
        }

        Commands::Leaderboard { server, n, limit } => {
            let client = reqwest::Client::new();
            let resp = client
                .get(format!("{server}/api/leaderboards/{n}?limit={limit}"))
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await?;
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
            } else {
                let status = resp.status();
                let text = resp.text().await?;
                anyhow::bail!("Query failed ({status}): {text}");
            }
        }

        Commands::Health { server } => {
            let client = reqwest::Client::new();
            let resp = client.get(format!("{server}/api/health")).send().await?;
            let data: serde_json::Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
    }

    Ok(())
}
