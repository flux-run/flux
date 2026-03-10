//! `flux schedule` — trigger functions or workflows on a cron schedule.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum ScheduleCommands {
    /// Create a new schedule
    Create {
        #[arg(long)]
        name: String,
        /// Standard 5-part cron expression (e.g. "0 2 * * *")
        #[arg(long)]
        cron: String,
        /// Function to trigger
        #[arg(long)]
        function: Option<String>,
        /// Workflow to trigger
        #[arg(long)]
        workflow: Option<String>,
        /// Static JSON payload
        #[arg(long)]
        payload: Option<String>,
    },
    /// List all schedules
    List,
    /// Pause a schedule (stops future runs)
    Pause {
        name: String,
    },
    /// Resume a paused schedule
    Resume {
        name: String,
    },
    /// Trigger a schedule immediately (one-off)
    Run {
        name: String,
    },
    /// Show run history for a schedule
    History {
        name: String,
    },
    /// Delete a schedule
    Delete {
        name: String,
        #[arg(long)]
        confirm: bool,
    },
}

pub async fn execute(command: ScheduleCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        ScheduleCommands::Create { name, cron, function, workflow, payload } => {
            if function.is_none() && workflow.is_none() {
                anyhow::bail!("Provide either --function or --workflow");
            }
            let static_payload: Value = payload
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid --payload JSON: {}", e))?
                .unwrap_or(serde_json::json!({}));

            let body = serde_json::json!({
                "name": name,
                "cron": cron,
                "function_name": function,
                "workflow_name": workflow,
                "payload": static_payload,
            });
            let res = client
                .client
                .post(format!("{}/schedules", client.base_url))
                .json(&body)
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!(
                "{} Scheduled: {}  cron: {}  next: {}",
                "✔".green().bold(),
                name.cyan(),
                cron.bold(),
                data["next_run"].as_str().unwrap_or("?").dimmed()
            );
        }

        ScheduleCommands::List => {
            let res = client
                .client
                .get(format!("{}/schedules", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let schedules = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if schedules.is_empty() {
                println!("No schedules.");
            } else {
                println!(
                    "{:<25} {:<20} {:<15} {}",
                    "NAME".bold(),
                    "CRON".bold(),
                    "STATUS".bold(),
                    "NEXT RUN".bold()
                );
                println!("{}", "─".repeat(80).dimmed());
                for s in schedules {
                    println!(
                        "{:<25} {:<20} {:<15} {}",
                        s["name"].as_str().unwrap_or(""),
                        s["cron"].as_str().unwrap_or(""),
                        s["status"].as_str().unwrap_or(""),
                        s["next_run"].as_str().unwrap_or("")
                    );
                }
            }
        }

        ScheduleCommands::Pause { name } => {
            let res = client
                .client
                .post(format!("{}/schedules/{}/pause", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Paused schedule '{}'", "✔".green().bold(), name.cyan());
        }

        ScheduleCommands::Resume { name } => {
            let res = client
                .client
                .post(format!("{}/schedules/{}/resume", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Resumed schedule '{}'", "✔".green().bold(), name.cyan());
        }

        ScheduleCommands::Run { name } => {
            let res = client
                .client
                .post(format!("{}/schedules/{}/run", client.base_url, name))
                .send()
                .await?;
            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let rid = data["request_id"].as_str().unwrap_or("?");
                println!("{} Triggered '{}' — request_id: {}", "✔".green().bold(), name.cyan(), rid.dimmed());
            } else {
                let status = res.status();
                anyhow::bail!("Failed to trigger schedule: {}", status);
            }
        }

        ScheduleCommands::History { name } => {
            let res = client
                .client
                .get(format!("{}/schedules/{}/history", client.base_url, name))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let history = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if history.is_empty() {
                println!("No history for '{}'.", name);
            } else {
                println!(
                    "{:<40} {:<10} {:<25} {}",
                    "RUN ID".bold(),
                    "STATUS".bold(),
                    "STARTED".bold(),
                    "DURATION".bold()
                );
                println!("{}", "─".repeat(85).dimmed());
                for h in history {
                    let run_id = h["id"].as_str().unwrap_or("");
                    let status = h["status"].as_str().unwrap_or("");
                    let started = h["started_at"].as_str().unwrap_or("");
                    let dur = h["duration_ms"].as_i64().unwrap_or(0);
                    println!("{:<40} {:<10} {:<25} {}ms", run_id, status, started, dur);
                }
            }
        }

        ScheduleCommands::Delete { name, confirm } => {
            if !confirm {
                print!("Delete schedule '{}'? [y/N]: ", name.red());
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
                .delete(format!("{}/schedules/{}", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted schedule '{}'", "✔".green().bold(), name);
        }
    }

    Ok(())
}
