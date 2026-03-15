//! `flux api-key` — manage programmatic API keys for CI/CD.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum ApiKeyCommands {
    /// Create a new API key
    Create {
        /// Descriptive name for this key
        #[arg(long)]
        name: String,
        /// Comma-separated permission scopes
        /// e.g. "function:deploy,logs:read"
        #[arg(long)]
        scopes: Option<String>,
    },
    /// List all API keys (secret values hidden)
    List,
    /// Revoke an API key immediately
    Revoke {
        id: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Rotate an API key (revoke old, issue new with same scopes)
    Rotate {
        id: String,
    },
}

const VALID_SCOPES: &[&str] = &[
    "function:invoke",
    "function:deploy",
    "logs:read",
    "secrets:write",
    "admin",
];

pub async fn execute(command: ApiKeyCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        ApiKeyCommands::Create { name, scopes } => {
            let scope_list: Vec<&str> = scopes
                .as_deref()
                .map(|s| s.split(',').map(str::trim).collect())
                .unwrap_or_else(|| vec!["function:invoke"]);

            // Validate scopes
            for s in &scope_list {
                if !VALID_SCOPES.contains(s) {
                    anyhow::bail!(
                        "Unknown scope '{}'. Valid scopes: {}",
                        s,
                        VALID_SCOPES.join(", ")
                    );
                }
            }

            let body = serde_json::json!({
                "name": name,
                "scopes": scope_list,
            });
            let res = client
                .client
                .post(R::api_keys::LIST.url(&client.base_url))
                .json(&body)
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let key = data["key"].as_str().unwrap_or("?");
                let id = data["id"].as_str().unwrap_or("?");
                println!(
                    "{} API key '{}' created",
                    "✔".green().bold(),
                    name.cyan()
                );
                println!("  id:     {}", id.dimmed());
                println!("  key:    {}  {}", key.bold().yellow(), "(store this — shown only once)".red().bold());
                println!("  scopes: {}", scope_list.join(", ").dimmed());
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Failed to create API key: {} — {}", status, body);
            }
        }

        ApiKeyCommands::List => {
            let res = client
                .client
                .get(R::api_keys::LIST.url(&client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let keys = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if keys.is_empty() {
                println!("No API keys.");
            } else {
                println!(
                    "{:<38} {:<25} {:<35} {}",
                    "ID".bold(),
                    "NAME".bold(),
                    "SCOPES".bold(),
                    "CREATED".bold()
                );
                println!("{}", "─".repeat(100).dimmed());
                for k in keys {
                    let scopes = k["scopes"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    println!(
                        "{:<38} {:<25} {:<35} {}",
                        k["id"].as_str().unwrap_or(""),
                        k["name"].as_str().unwrap_or(""),
                        scopes,
                        k["created_at"].as_str().unwrap_or("")
                    );
                }
            }
        }

        ApiKeyCommands::Revoke { id, confirm } => {
            if !confirm {
                print!("Revoke API key {}? This is immediate and irreversible. [y/N]: ", id.red());
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
                .delete(R::api_keys::DELETE.url_with(&client.base_url, &[("id", &id)]))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Revoked API key {}", "✔".green().bold(), id.dimmed());
        }

        ApiKeyCommands::Rotate { id } => {
            let res = client
                .client
                .post(R::api_keys::ROTATE.url_with(&client.base_url, &[("id", &id)]))
                .send()
                .await?;
            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let new_key = data["key"].as_str().unwrap_or("?");
                let new_id = data["id"].as_str().unwrap_or("?");
                println!("{} API key rotated", "✔".green().bold());
                println!("  old id: {}  (revoked)", id.dimmed());
                println!("  new id: {}", new_id.dimmed());
                println!("  new key: {}  {}", new_key.bold().yellow(), "(store this — shown only once)".red().bold());
            } else {
                let status = res.status();
                anyhow::bail!("Rotate failed: {}", status);
            }
        }
    }

    Ok(())
}
