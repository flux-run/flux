//! `flux secret` — manage local development secrets in `.env.local`.
//!
//! Secrets are stored in `.env.local` (gitignored by default) in the current
//! project directory.  `flux dev` auto-loads this file into the function
//! environment so secrets are available to your functions without being
//! committed to source control.

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
    match command {
        SecretsCommands::List => cmd_list(),
        SecretsCommands::Get { key } => cmd_get(&key),
        SecretsCommands::Set { key, value } => cmd_set(&key, &value),
        SecretsCommands::Delete { key } => cmd_delete(&key),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
