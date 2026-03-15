//! `flux config` — inspect and modify config without editing JSON manually.
//!
//! ```text
//! flux config list              # show all active config values
//! flux config set <key> <val>   # set a value in the nearest config file
//! flux config reset             # restore defaults (prompts confirmation)
//! ```

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;

use crate::config::{Config, ProjectConfig};

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show all active configuration values
    List,
    /// Set a configuration value
    Set {
        /// Configuration key (e.g. api_url, gateway_url, runtime_url, project_id)
        key: String,
        /// Value to set
        value: String,
    },
    /// Reset configuration to platform defaults
    Reset,
}

pub async fn execute(command: ConfigCommands) -> anyhow::Result<()> {
    match command {
        ConfigCommands::List => cmd_list().await,
        ConfigCommands::Set { key, value } => cmd_set(key, value).await,
        ConfigCommands::Reset => cmd_reset().await,
    }
}

// ── list ─────────────────────────────────────────────────────────────────────

async fn cmd_list() -> anyhow::Result<()> {
    let (source, values) = load_effective_config();

    println!();
    println!("  Source: {}", source.dimmed());
    println!();

    let key_w = 18usize;
    println!(
        "  {}  {}",
        format!("{:<key_w$}", "KEY").bold(),
        "VALUE".bold()
    );
    println!("  {}", "─".repeat(65).dimmed());

    for (k, v) in &values {
        println!("  {:<key_w$}  {}", k, v.dimmed());
    }
    println!();

    Ok(())
}

fn load_effective_config() -> (String, Vec<(String, String)>) {
    // Check for per-project config first
    if let Some(proj_path) = ProjectConfig::find_path_pub() {
        if let Ok(src) = std::fs::read_to_string(&proj_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&src) {
                let mut vals = json_to_kv(&json);
                // Merge in global config keys that are missing
                if let Some(home) = global_config_path() {
                    if let Ok(gsrc) = std::fs::read_to_string(&home) {
                        if let Ok(gjson) = serde_json::from_str::<Value>(&gsrc) {
                            for (k, v) in json_to_kv(&gjson) {
                                if !vals.iter().any(|(ek, _)| ek == &k) {
                                    vals.push((k, format!("{} (global)", v)));
                                }
                            }
                        }
                    }
                }
                let display_path = proj_path.display().to_string();
                return (display_path, vals);
            }
        }
    }

    // Fall back to global config
    if let Some(path) = global_config_path() {
        if let Ok(src) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<Value>(&src) {
                let display = path.display().to_string();
                return (display, json_to_kv(&json));
            }
        }
    }

    ("(no config found)".to_string(), vec![])
}

fn json_to_kv(json: &Value) -> Vec<(String, String)> {
    let mut out = vec![];
    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Null => "(null)".to_string(),
                other => other.to_string(),
            };
            out.push((k.clone(), val));
        }
    }
    out
}

fn global_config_path() -> Option<PathBuf> {
    let mut p = dirs::home_dir()?;
    p.push(".flux");
    p.push("config.json");
    Some(p)
}

// ── set ──────────────────────────────────────────────────────────────────────

async fn cmd_set(key: String, value: String) -> anyhow::Result<()> {
    // Determine which file to write to
    let target_path = if ProjectConfig::find_path_pub().is_some() {
        ProjectConfig::find_path_pub().unwrap()
    } else {
        global_config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
    };

    let existing = if target_path.exists() {
        let src = fs::read_to_string(&target_path).await?;
        serde_json::from_str::<Value>(&src).unwrap_or(Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };

    let mut obj = match existing {
        Value::Object(m) => m,
        _ => Default::default(),
    };

    // Validate known keys
    const KNOWN: &[&str] = &[
        "api_url",
        "gateway_url",
        "runtime_url",
        "project_id",
        "tenant_id",
        "tenant_slug",
        "sdk_output",
        "watch_interval",
    ];
    if !KNOWN.contains(&key.as_str()) {
        eprintln!(
            "{} Unknown key '{}'. Known keys: {}",
            "⚠".yellow().bold(),
            key,
            KNOWN.join(", ")
        );
    }

    obj.insert(key.clone(), Value::String(value.clone()));

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(obj))?;
    fs::write(&target_path, json).await?;

    println!(
        "{} Set {} = {}  in {}",
        "✔".green().bold(),
        key.bold(),
        value.cyan(),
        target_path.display().to_string().dimmed()
    );
    Ok(())
}

// ── reset ─────────────────────────────────────────────────────────────────────

async fn cmd_reset() -> anyhow::Result<()> {
    // Determine which file
    let target_path = if let Some(p) = ProjectConfig::find_path_pub() {
        p
    } else {
        global_config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
    };

    print!(
        "  This will restore all values in {} to platform defaults.\n  Confirm? [y/N]: ",
        target_path.display().to_string().cyan()
    );
    use std::io::{self, BufRead, Write};
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;

    if line.trim().to_lowercase() != "y" {
        println!("Aborted.");
        return Ok(());
    }

    // Build default config
    let default = Config::default();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "api_url": default.api_url,
        "gateway_url": default.gateway_url,
        "runtime_url": default.runtime_url,
    }))?;
    fs::write(&target_path, json).await?;

    println!(
        "{} Reset {}",
        "✔".green().bold(),
        target_path.display().to_string().dimmed()
    );
    Ok(())
}
