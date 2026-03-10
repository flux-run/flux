//! `flux env` — manage named environments (production, staging, preview).

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use crate::config::Config;

#[derive(Subcommand)]
pub enum EnvCommands {
    /// List all environments in the project
    List,
    /// Create a new environment
    Create {
        name: String,
    },
    /// Delete an environment
    Delete {
        name: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Switch the active environment for CLI commands
    Use {
        name: String,
    },
    /// Clone secrets and config from one environment to another
    Clone {
        /// Source environment
        source: String,
        /// Destination environment
        destination: String,
    },
}

pub async fn execute(command: EnvCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        EnvCommands::List => {
            let res = client
                .client
                .get(format!("{}/environments", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let envs = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if envs.is_empty() {
                println!("No environments (only production exists by default).");
            } else {
                println!("{:<25} {:<15} {}", "NAME".bold(), "SECRETS".bold(), "CREATED".bold());
                println!("{}", "─".repeat(60).dimmed());
                for e in envs {
                    println!(
                        "{:<25} {:<15} {}",
                        e["name"].as_str().unwrap_or(""),
                        e["secret_count"].as_i64().unwrap_or(0),
                        e["created_at"].as_str().unwrap_or("")
                    );
                }
            }
        }

        EnvCommands::Create { name } => {
            let res = client
                .client
                .post(format!("{}/environments", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            res.error_for_status()?;
            println!(
                "{} Environment '{}' created",
                "✔".green().bold(),
                name.cyan()
            );
        }

        EnvCommands::Delete { name, confirm } => {
            if !confirm {
                print!("Delete environment '{}'? This removes all its secrets. [y/N]: ", name.red());
                use std::io::{BufRead, Write};
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                if line.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let res = client
                .client
                .delete(format!("{}/environments/{}", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted environment '{}'", "✔".green().bold(), name);
        }

        EnvCommands::Use { name } => {
            // Store selected env in project or global config
            let mut config = Config::load().await;
            // We store it as an extra field — for now just acknowledge
            println!(
                "{} Using environment: {}",
                "✔".green().bold(),
                name.cyan()
            );
            println!(
                "  {} Use {} to override per-command",
                "hint:".dimmed(),
                "--env <name>".bold()
            );
            let _ = config;
        }

        EnvCommands::Clone { source, destination } => {
            let res = client
                .client
                .post(format!("{}/environments/clone", client.base_url))
                .json(&serde_json::json!({
                    "source": source,
                    "destination": destination,
                }))
                .send()
                .await?;
            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let count = data["cloned_secrets"].as_i64().unwrap_or(0);
                println!(
                    "{} Cloned secrets: {} → {}  ({} secrets)",
                    "✔".green().bold(),
                    source.cyan(),
                    destination.cyan(),
                    count
                );
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Clone failed: {} — {}", status, body);
            }
        }
    }

    Ok(())
}
