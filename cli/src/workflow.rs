//! `flux workflow` — define and run multi-step orchestration workflows.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum WorkflowCommands {
    /// Create a workflow definition
    Create {
        /// Workflow name
        name: String,
    },
    /// List workflows in the current project
    List,
    /// Get details of a workflow
    Get {
        /// Workflow name
        name: String,
    },
    /// Deploy a workflow definition
    Deploy {
        /// Workflow name
        name: String,
    },
    /// Run a workflow
    Run {
        /// Workflow name
        name: String,
        /// JSON payload to pass to the workflow
        #[arg(long, value_name = "JSON")]
        payload: Option<String>,
    },
    /// Show logs for a workflow execution
    Logs {
        /// Workflow name
        name: String,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Show the trace for a workflow execution
    Trace {
        /// Workflow name
        name: String,
        /// Request ID for a specific run
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Delete a workflow
    Delete {
        /// Workflow name
        name: String,
        #[arg(long)]
        confirm: bool,
    },
}

pub async fn execute(command: WorkflowCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        WorkflowCommands::Create { name } => {
            let res = client
                .client
                .post(format!("{}/workflows", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!(
                "{} Workflow '{}' created (id: {})",
                "✔".green().bold(),
                name.cyan(),
                data["id"].as_str().unwrap_or("?").dimmed()
            );
        }

        WorkflowCommands::List => {
            let res = client
                .client
                .get(format!("{}/workflows", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let workflows = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if workflows.is_empty() {
                println!("No workflows defined.");
            } else {
                println!("{:<30} {:<15} {}", "NAME".bold(), "STATUS".bold(), "UPDATED".bold());
                println!("{}", "─".repeat(70).dimmed());
                for w in workflows {
                    let name = w["name"].as_str().unwrap_or("");
                    let status = w["status"].as_str().unwrap_or("unknown");
                    let updated = w["updated_at"].as_str().unwrap_or("");
                    println!("{:<30} {:<15} {}", name, status, updated);
                }
            }
        }

        WorkflowCommands::Get { name } => {
            let res = client
                .client
                .get(format!("{}/workflows/{}", client.base_url, name))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!("{}", serde_json::to_string_pretty(data)?);
        }

        WorkflowCommands::Deploy { name } => {
            let res = client
                .client
                .post(format!("{}/workflows/{}/deploy", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Workflow '{}' deployed", "✔".green().bold(), name.cyan());
        }

        WorkflowCommands::Run { name, payload } => {
            let body: Value = payload
                .as_deref()
                .map(|s| serde_json::from_str(s))
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid --payload JSON: {}", e))?
                .unwrap_or(serde_json::json!({}));

            let t0 = std::time::Instant::now();
            let res = client
                .client
                .post(format!("{}/workflows/{}/run", client.base_url, name))
                .json(&body)
                .send()
                .await?;
            let elapsed = t0.elapsed().as_millis();

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let request_id = data["request_id"].as_str().unwrap_or("?");
                println!(
                    "{} Workflow '{}' started ({}ms)",
                    "✔".green().bold(),
                    name.cyan(),
                    elapsed
                );
                println!("  request_id: {}", request_id.dimmed());
                // Print step results if available
                if let Some(steps) = data["steps"].as_array() {
                    for step in steps {
                        let step_name = step["name"].as_str().unwrap_or("?");
                        let step_status = step["status"].as_str().unwrap_or("?");
                        let step_ms = step["duration_ms"].as_i64().unwrap_or(0);
                        let marker = if step_status == "success" {
                            "✔".green().bold().to_string()
                        } else if step_status == "pending" {
                            "⏳".yellow().to_string()
                        } else {
                            "✗".red().bold().to_string()
                        };
                        println!("  step {:<20} {}  {}ms", step_name, marker, step_ms);
                    }
                }
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Workflow run failed ({}ms): {} — {}", elapsed, status, body);
            }
        }

        WorkflowCommands::Logs { name, request_id } => {
            let mut url = format!("{}/logs?source=workflow&resource={}&limit=100", client.base_url, name);
            if let Some(rid) = &request_id {
                url.push_str(&format!("&request_id={}", rid));
            }
            let res = client.client.get(&url).send().await?;
            let json: Value = res.error_for_status()?.json().await?;
            let logs = json
                .get("data")
                .and_then(|d| d.get("logs"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            for entry in logs {
                let ts = entry["timestamp"].as_str().map(|t| &t[..t.len().min(19)]).unwrap_or("?");
                let msg = entry["message"].as_str().unwrap_or("");
                let level = entry["level"].as_str().unwrap_or("info");
                println!("[{}] {} {}", ts.dimmed(), level.to_uppercase().cyan(), msg);
            }
        }

        WorkflowCommands::Trace { name, request_id } => {
            let rid = request_id.ok_or_else(|| anyhow::anyhow!(
                "Provide --request-id to view a specific workflow trace"
            ))?;
            let res = client
                .client
                .get(format!("{}/workflows/{}/traces/{}", client.base_url, name, rid))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!("{}", serde_json::to_string_pretty(data)?);
        }

        WorkflowCommands::Delete { name, confirm } => {
            if !confirm {
                print!("Delete workflow '{}'? [y/N]: ", name.red());
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
                .delete(format!("{}/workflows/{}", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted workflow '{}'", "✔".green().bold(), name);
        }
    }

    Ok(())
}
