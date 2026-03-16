use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod identity;

#[derive(Parser)]
#[command(
    name = "minegraph",
    about = "MineGraph CLI — graph search, identity, and experiments"
)]
struct Cli {
    /// Path to project config directory (default: .config/minegraph/ in cwd)
    #[arg(long, global = true)]
    config_dir: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new Ed25519 signing keypair
    Keygen {
        /// Display name for this key (optional)
        #[arg(long)]
        name: Option<String>,
    },
    /// Show current identity (key_id and display name)
    Whoami,
    /// Show or edit config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Initialize a MineGraph config directory in the current project
    Init,
    /// Register your public key with a MineGraph server
    RegisterKey {
        /// Server URL (uses config server_url if not specified)
        #[arg(long)]
        server: Option<String>,
        /// Display name (uses key file display_name if not specified)
        #[arg(long)]
        name: Option<String>,
        /// GitHub repo (e.g., "user/repo")
        #[arg(long)]
        github_repo: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current config
    Show,
    /// Set a config value
    Set {
        /// Key (e.g., server_url, strategy, beam_width)
        key: String,
        /// Value
        value: String,
    },
    /// Get a config value
    Get {
        /// Key
        key: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let config_dir = config::resolve_config_dir(cli.config_dir.as_deref());

    match cli.command {
        Command::Init => {
            config::init_config_dir(&config_dir)?;
            println!("Initialized MineGraph config at {}", config_dir.display());
            println!("\nRun `minegraph keygen` to create a signing key.");
        }

        Command::Keygen { name } => {
            config::ensure_config_dir(&config_dir)?;
            let key_path = config_dir.join("key.json");
            if key_path.exists() {
                eprintln!("Key already exists at {}", key_path.display());
                eprintln!("To generate a new key, remove it first.");
                std::process::exit(1);
            }
            let info = identity::generate_and_save(&key_path, name.as_deref())?;
            println!("Generated new signing key:");
            println!("  Key ID:  {}", info.key_id);
            if let Some(ref n) = info.display_name {
                println!("  Name:    {}", n);
            }
            println!("  Saved:   {}", key_path.display());
            println!("\nYour public key (share this):");
            println!("  {}", info.public_key_hex);
        }

        Command::Whoami => {
            let key_path = config_dir.join("key.json");
            if !key_path.exists() {
                println!("No signing key found.");
                println!("Run `minegraph keygen` to create one.");
                std::process::exit(0);
            }
            let info = identity::load_key_info(&key_path)?;
            println!("Key ID:     {}", info.key_id);
            if let Some(ref n) = info.display_name {
                println!("Name:       {}", n);
            }
            println!("Public key: {}", info.public_key_hex);
            println!("Key file:   {}", key_path.display());
        }

        Command::RegisterKey {
            server,
            name,
            github_repo,
        } => {
            let key_path = config_dir.join("key.json");
            if !key_path.exists() {
                eprintln!("No signing key found. Run `minegraph keygen` first.");
                std::process::exit(1);
            }
            let info = identity::load_key_info(&key_path)?;
            let config_path = config_dir.join("config.toml");
            let cfg = config::load_config(&config_path)?;
            let server_url = server.unwrap_or(cfg.server_url);
            let display_name = name.or(info.display_name);

            let url = format!("{}/api/keys", server_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "public_key": info.public_key_hex,
                "display_name": display_name,
                "github_repo": github_repo,
            });

            println!("Registering key {} with {}...", info.key_id, server_url);
            let resp = reqwest::blocking::Client::new()
                .post(&url)
                .json(&body)
                .send()
                .context("Failed to connect to server")?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json()?;
                println!("Registered!");
                println!("  Key ID:       {}", data["key_id"]);
                if let Some(n) = data["display_name"].as_str() {
                    println!("  Display name: {}", n);
                }
                if let Some(r) = data["github_repo"].as_str() {
                    println!("  GitHub repo:  {}", r);
                }
            } else {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                eprintln!("Registration failed: {} {}", status, body);
                std::process::exit(1);
            }
        }

        Command::Config { action } => {
            config::ensure_config_dir(&config_dir)?;
            let config_path = config_dir.join("config.toml");
            match action {
                ConfigAction::Show => {
                    let cfg = config::load_config(&config_path)?;
                    println!("{}", toml::to_string_pretty(&cfg)?);
                }
                ConfigAction::Set { key, value } => {
                    let mut cfg = config::load_config(&config_path)?;
                    config::set_value(&mut cfg, &key, &value)?;
                    config::save_config(&config_path, &cfg)?;
                    println!("{} = {}", key, value);
                }
                ConfigAction::Get { key } => {
                    let cfg = config::load_config(&config_path)?;
                    match config::get_value(&cfg, &key) {
                        Some(v) => println!("{}", v),
                        None => {
                            eprintln!("Key '{}' not set", key);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
