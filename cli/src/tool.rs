//! `flux tool` — manage external tool integrations via Composio.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum ToolCommands {
    /// List all available tools
    List,
    /// Search tools by keyword
    Search {
        /// Search query (e.g. "send email")
        query: String,
    },
    /// Describe a tool action and its parameters
    Describe {
        /// Tool action (e.g. gmail.send_email)
        tool: String,
    },
    /// Connect an external app (OAuth or API key flow)
    Connect {
        /// App name (e.g. gmail, slack, github)
        app: String,
    },
    /// Disconnect a connected app
    Disconnect {
        /// App name
        app: String,
    },
    /// Run a tool action directly from the terminal
    Run {
        /// Tool action (e.g. gmail.send_email)
        action: String,
        /// Parameters as key=value pairs (repeatable)
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
    },
}

pub async fn execute(command: ToolCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        ToolCommands::List => {
            let res = client
                .client
                .get(format!("{}/tools", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let tools = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if tools.is_empty() {
                println!("No tools connected.");
                println!("  {}", "flux tool connect <app>".dimmed());
            } else {
                println!(
                    "{:<15} {:<35} {}",
                    "APP".bold(),
                    "ACTION".bold(),
                    "DESCRIPTION".bold()
                );
                println!("{}", "─".repeat(80).dimmed());
                for t in tools {
                    let app = t["app"].as_str().unwrap_or("");
                    let action = t["action"].as_str().unwrap_or("");
                    let desc = t["description"].as_str().unwrap_or("");
                    println!("{:<15} {:<35} {}", app, action, desc);
                }
            }
        }

        ToolCommands::Search { query } => {
            let res = client
                .client
                .get(format!(
                    "{}/tools/search?q={}",
                    client.base_url,
                    urlencoding::encode(&query)
                ))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let results = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if results.is_empty() {
                println!("No tools found matching '{}'.", query);
            } else {
                for r in results {
                    let action = r["action"].as_str().unwrap_or("");
                    let desc = r["description"].as_str().unwrap_or("");
                    println!("{}  {}", action.cyan().bold(), desc.dimmed());
                }
            }
        }

        ToolCommands::Describe { tool } => {
            let res = client
                .client
                .get(format!("{}/tools/{}", client.base_url, tool))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);

            let name = data["action"].as_str().unwrap_or(&tool);
            let desc = data["description"].as_str().unwrap_or("");
            let params = data["parameters"].as_array();

            println!();
            println!("  {}", name.cyan().bold());
            if !desc.is_empty() {
                println!("  {}", desc);
            }
            println!();
            if let Some(params) = params {
                println!("  {}", "Parameters:".bold());
                for p in params {
                    let pname = p["name"].as_str().unwrap_or("");
                    let ptype = p["type"].as_str().unwrap_or("string");
                    let req = if p["required"].as_bool().unwrap_or(false) {
                        "required"
                    } else {
                        "optional"
                    };
                    println!("    {:<25} {:<10} {}", pname.bold(), ptype, req.dimmed());
                }
            }
            println!();
        }

        ToolCommands::Connect { app } => {
            println!(
                "  Opening browser to connect {}…",
                app.cyan().bold()
            );

            let res = client
                .client
                .post(format!("{}/tools/connect", client.base_url))
                .json(&serde_json::json!({ "app": app }))
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                if let Some(url) = data["auth_url"].as_str() {
                    let _ = open::that(url);
                    println!("  Waiting for authorization…");
                    println!("  Authorisation URL: {}", url.dimmed());
                } else {
                    println!("{} Connected: {}", "✔".green().bold(), app.cyan());
                }
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Failed to connect {}: {} — {}", app, status, body);
            }
        }

        ToolCommands::Disconnect { app } => {
            print!("Disconnect {}? This will remove stored credentials. [y/N]: ", app.red());
            use std::io::{BufRead, Write};
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            if line.trim().to_lowercase() != "y" {
                println!("Aborted.");
                return Ok(());
            }

            let res = client
                .client
                .delete(format!("{}/tools/connect/{}", client.base_url, app))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Disconnected {}", "✔".green().bold(), app.cyan());
        }

        ToolCommands::Run { action, params } => {
            // Parse key=value pairs
            let mut param_map = serde_json::Map::new();
            for p in &params {
                let mut parts = p.splitn(2, '=');
                let k = parts.next().unwrap_or("").to_string();
                let v = parts.next().unwrap_or("").to_string();
                param_map.insert(k, Value::String(v));
            }

            println!("  Running {}…", action.cyan().bold());
            let t0 = std::time::Instant::now();

            let res = client
                .client
                .post(format!("{}/tools/run", client.base_url))
                .json(&serde_json::json!({
                    "action": action,
                    "params": Value::Object(param_map),
                }))
                .send()
                .await?;

            let elapsed = t0.elapsed().as_millis();
            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                println!(
                    "{} {} completed ({}ms)",
                    "✔".green().bold(),
                    action.cyan(),
                    elapsed
                );
                println!("{}", serde_json::to_string_pretty(data)?);
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("{} failed ({}ms): {} — {}", action, elapsed, status, body);
            }
        }
    }

    Ok(())
}

// Minimal URL encoding for query strings
mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .flat_map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                    vec![c]
                } else {
                    format!("%{:02X}", c as u8).chars().collect()
                }
            })
            .collect()
    }
}
