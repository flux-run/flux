//! `flux secret` — manage secrets via API or local `.env.local`.
//!
//! When `FLUX_API_URL` and `FLUX_TOKEN` are both set, secrets are
//! managed through the API.  Otherwise they are stored in `.env.local`
//! (gitignored by default) in the current project directory.

use clap::Subcommand;
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::Path;

const ENV_FILE: &str = ".env.local";

#[derive(Subcommand)]
pub enum SecretsCommands {
    /// List all secrets (values are redacted)
    List,
    /// Read the value of a secret
    Get {
        key: String,
    },
    /// Set (create or update) a secret
    Set {
        key: String,
        value: String,
    },
    /// Delete a secret
    Delete {
        key: String,
    },
}

pub async fn execute(command: SecretsCommands) -> anyhow::Result<()> {
    let api_url   = std::env::var("FLUX_API_URL").ok();
    let api_token = std::env::var("FLUX_TOKEN").ok();

    if api_url.is_some() && api_token.is_some() {
        execute_api(command, api_url.unwrap(), api_token.unwrap()).await
    } else {
        match command {
            SecretsCommands::List          => cmd_list(),
            SecretsCommands::Get { key }   => cmd_get(&key),
            SecretsCommands::Set { key, value } => cmd_set(&key, &value),
            SecretsCommands::Delete { key } => cmd_delete(&key),
        }
    }
}

// ── API mode ──────────────────────────────────────────────────────────────────

async fn execute_api(command: SecretsCommands, api_url: String, token: String) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    match command {
        SecretsCommands::List => {
            let res = client
                .get(format!("{}/secrets", api_url))
                .bearer_auth(&token)
                .send()
                .await?;
            let secrets: serde_json::Value = res.error_for_status()?.json().await?;
            if let Some(arr) = secrets.as_array() {
                println!("{:<30} {}", "KEY", "VERSION");
                println!("{}", "-".repeat(40));
                for s in arr {
                    let key     = s.get("key").and_then(|v| v.as_str()).unwrap_or("");
                    let version = s.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
                    println!("{:<30} {}", key, version);
                }
            }
        }
        SecretsCommands::Get { key } => {
            let res = client
                .get(format!("{}/secrets/{}", api_url, key))
                .bearer_auth(&token)
                .send()
                .await?;
            let secret: serde_json::Value = res.error_for_status()?.json().await?;
            println!("{}", serde_json::to_string_pretty(&secret)?);
        }
        SecretsCommands::Set { key, value } => {
            let res = client
                .post(format!("{}/secrets", api_url))
                .bearer_auth(&token)
                .json(&serde_json::json!({ "key": key, "value": value }))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Set secret '{}'", "✔".green().bold(), key.cyan());
        }
        SecretsCommands::Delete { key } => {
            let res = client
                .delete(format!("{}/secrets/{}", api_url, key))
                .bearer_auth(&token)
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted secret '{}'", "✔".green().bold(), key.cyan());
        }
    }
    Ok(())
}

// ── Local .env.local mode ────────────────────────────────────────────────────

/// Parse `.env.local` into an ordered map, preserving comment/blank lines as-is.
fn load_env() -> anyhow::Result<BTreeMap<String, String>> {
    let path = Path::new(ENV_FILE);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw = std::fs::read_to_string(path)?;
    let mut map = BTreeMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}

/// Rewrite `.env.local` from a BTreeMap (sorted, one KEY=VALUE per line).
fn save_env(map: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let mut lines = Vec::new();
    for (k, v) in map {
        lines.push(format!("{}={}", k, v));
    }
    std::fs::write(ENV_FILE, lines.join("\n") + "\n")?;
    Ok(())
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_list() -> anyhow::Result<()> {
    let map = load_env()?;
    if map.is_empty() {
        println!("No secrets found in {}.", ENV_FILE.bold());
        println!("  Use {} to add one.", "flux secret set KEY VALUE".bold());
        return Ok(());
    }
    println!("{}", ENV_FILE.bold());
    println!("{}", "─".repeat(40).dimmed());
    for key in map.keys() {
        println!("  {}  {}", key.cyan(), "(redacted)".dimmed());
    }
    println!();
    println!("  {} {} to read a value", "tip:".dimmed(), "flux secret get KEY".bold());
    Ok(())
}

fn cmd_get(key: &str) -> anyhow::Result<()> {
    let map = load_env()?;
    match map.get(key) {
        Some(v) => {
            println!("{}", v);
            Ok(())
        }
        None => anyhow::bail!("Secret '{}' not found in {}", key, ENV_FILE),
    }
}

fn cmd_set(key: &str, value: &str) -> anyhow::Result<()> {
    let mut map = load_env()?;
    let is_update = map.contains_key(key);
    map.insert(key.to_string(), value.to_string());
    save_env(&map)?;
    if is_update {
        println!("{} Updated secret '{}'", "✔".green().bold(), key.cyan());
    } else {
        println!("{} Set secret '{}'  (stored in {})", "✔".green().bold(), key.cyan(), ENV_FILE.dimmed());
    }
    Ok(())
}

fn cmd_delete(key: &str) -> anyhow::Result<()> {
    let mut map = load_env()?;
    if map.remove(key).is_none() {
        anyhow::bail!("Secret '{}' not found in {}", key, ENV_FILE);
    }
    save_env(&map)?;
    println!("{} Deleted secret '{}'", "✔".green().bold(), key.cyan());
    Ok(())
}
