//! `flux agent` — define and run AI agents that reason, plan, and call tools.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Create a new agent definition
    Create {
        name: String,
    },
    /// List agents in the current project
    List,
    /// Get details of an agent
    Get {
        name: String,
    },
    /// Deploy an agent
    Deploy {
        name: String,
    },
    /// Run an agent with a natural-language input
    Run {
        name: String,
        /// Input text for the agent
        #[arg(long)]
        input: Option<String>,
    },
    /// Run the agent against a fixture scenario safely (no real tool calls)
    Simulate {
        name: String,
        /// Scenario file path (JSON)
        #[arg(long)]
        scenario: Option<String>,
    },
    /// Replay the recorded reasoning trace for a past run
    Trace {
        name: String,
        /// Request ID for the run to inspect
        #[arg(long)]
        request_id: String,
    },
    /// Delete an agent
    Delete {
        name: String,
        #[arg(long)]
        confirm: bool,
    },
}

pub async fn execute(command: AgentCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        AgentCommands::Create { name } => {
            let res = client
                .client
                .post(format!("{}/agents", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!(
                "{} Agent '{}' created (id: {})",
                "✔".green().bold(),
                name.cyan(),
                data["id"].as_str().unwrap_or("?").dimmed()
            );
        }

        AgentCommands::List => {
            let res = client
                .client
                .get(format!("{}/agents", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let agents = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if agents.is_empty() {
                println!("No agents defined.");
            } else {
                println!(
                    "{:<30} {:<15} {}",
                    "NAME".bold(),
                    "STATUS".bold(),
                    "UPDATED".bold()
                );
                println!("{}", "─".repeat(65).dimmed());
                for a in agents {
                    println!(
                        "{:<30} {:<15} {}",
                        a["name"].as_str().unwrap_or(""),
                        a["status"].as_str().unwrap_or("unknown"),
                        a["updated_at"].as_str().unwrap_or("")
                    );
                }
            }
        }

        AgentCommands::Get { name } => {
            let res = client
                .client
                .get(format!("{}/agents/{}", client.base_url, name))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            println!("{}", serde_json::to_string_pretty(json.get("data").unwrap_or(&json))?);
        }

        AgentCommands::Deploy { name } => {
            let res = client
                .client
                .post(format!("{}/agents/{}/deploy", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Agent '{}' deployed", "✔".green().bold(), name.cyan());
        }

        AgentCommands::Run { name, input } => {
            let body = serde_json::json!({ "input": input.unwrap_or_default() });
            let t0 = std::time::Instant::now();
            let res = client
                .client
                .post(format!("{}/agents/{}/run", client.base_url, name))
                .json(&body)
                .send()
                .await?;
            let elapsed = t0.elapsed().as_millis();

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);

                if let Some(steps) = data["steps"].as_array() {
                    for step in steps {
                        let tool = step["tool"].as_str().unwrap_or("?");
                        let step_ms = step["duration_ms"].as_i64().unwrap_or(0);
                        let ok = step["success"].as_bool().unwrap_or(true);
                        let marker = if ok { "✔".green().bold().to_string() } else { "✗".red().bold().to_string() };
                        let note = step["note"].as_str().unwrap_or("");
                        println!("  {} tool: {}  {}ms  {}", marker, tool.cyan(), step_ms, note.dimmed());
                    }
                }

                let result = data["result"].as_str().unwrap_or("");
                let steps_n = data["steps"].as_array().map(|a| a.len()).unwrap_or(0);
                println!(
                    "  {} Done ({} steps, {}ms)",
                    "✔".green().bold(),
                    steps_n,
                    elapsed
                );
                if !result.is_empty() {
                    println!("  Result: \"{}\"", result);
                }
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Agent run failed ({}ms): {} — {}", elapsed, status, body);
            }
        }

        AgentCommands::Simulate { name, scenario } => {
            let body = if let Some(path) = &scenario {
                let raw = tokio::fs::read_to_string(path).await?;
                serde_json::from_str::<Value>(&raw)
                    .map_err(|e| anyhow::anyhow!("Invalid scenario JSON: {}", e))?
            } else {
                serde_json::json!({})
            };

            println!("  Simulating agent '{}' (no real tool calls)…", name.cyan());
            let res = client
                .client
                .post(format!("{}/agents/{}/simulate", client.base_url, name))
                .json(&body)
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                if let Some(steps) = data["steps"].as_array() {
                    for step in steps {
                        let tool = step["tool"].as_str().unwrap_or("?");
                        let note = step["note"].as_str().unwrap_or("");
                        println!("  → tool: {}  {}", tool.cyan(), note.dimmed());
                    }
                }
                println!(
                    "  {} Simulation complete",
                    "✔".green().bold()
                );
            } else {
                let status = res.status();
                println!("  {} Simulation failed: {}", "✗".red().bold(), status);
            }
        }

        AgentCommands::Trace { name, request_id } => {
            let res = client
                .client
                .get(format!("{}/agents/{}/traces/{}", client.base_url, name, request_id))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);

            println!();
            if let Some(steps) = data["steps"].as_array() {
                for (i, step) in steps.iter().enumerate() {
                    let tool = step["tool"].as_str().unwrap_or("?");
                    let ms = step["duration_ms"].as_i64().unwrap_or(0);
                    let ok = step["success"].as_bool().unwrap_or(true);
                    let note = step["note"].as_str().unwrap_or("");
                    let marker = if ok { "✔".green().bold().to_string() } else { "✗".red().bold().to_string() };
                    println!(
                        "  step {}  {:<35} {}ms  {}  {}",
                        i + 1,
                        tool.cyan(),
                        ms,
                        marker,
                        note.dimmed()
                    );
                }
            }
            if let Some(conclusion) = data["conclusion"].as_str() {
                println!();
                println!("  conclusion: \"{}\"", conclusion);
            }
            println!();
        }

        AgentCommands::Delete { name, confirm } => {
            if !confirm {
                print!("Delete agent '{}'? [y/N]: ", name.red());
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
                .delete(format!("{}/agents/{}", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted agent '{}'", "✔".green().bold(), name);
        }
    }

    Ok(())
}
