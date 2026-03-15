//! `flux queue` — manage async message queues with retry and dead-letter support.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum QueueCommands {
    /// Create a new queue
    Create {
        name: String,
        /// Maximum delivery attempts before moving to DLQ
        #[arg(long, default_value = "3")]
        max_retries: u32,
        /// Visibility timeout (e.g. 30s, 5m)
        #[arg(long, default_value = "30s")]
        visibility_timeout: String,
    },
    /// List all queues in the project
    List,
    /// Show queue stats and configuration
    Describe {
        name: String,
    },
    /// Publish a message to a queue
    Publish {
        name: String,
        /// JSON payload for the message
        #[arg(long, value_name = "JSON")]
        payload: String,
    },
    /// Bind a function as a consumer of this queue
    Bind {
        name: String,
        /// Function name to bind as consumer
        #[arg(long)]
        function: String,
    },
    /// List bindings (consumers) for a queue
    Bindings {
        name: String,
    },
    /// Purge all messages from a queue
    Purge {
        name: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Delete a queue
    Delete {
        name: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Dead-letter queue operations
    Dlq {
        #[command(subcommand)]
        command: DlqCommands,
    },
}

#[derive(Subcommand)]
pub enum DlqCommands {
    /// List dead-lettered messages
    List {
        /// Queue name
        name: String,
    },
    /// Replay dead-lettered messages back to the main queue
    Replay {
        /// Queue name
        name: String,
    },
}

pub async fn execute(command: QueueCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        QueueCommands::Create { name, max_retries, visibility_timeout } => {
            let body = serde_json::json!({
                "name": name,
                "max_retries": max_retries,
                "visibility_timeout": visibility_timeout,
            });
            let res = client
                .client
                .post(R::queues::LIST.url(&client.base_url))
                .json(&body)
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!(
                "{} Queue '{}' created  (max_retries: {}, timeout: {})",
                "✔".green().bold(),
                name.cyan(),
                max_retries,
                visibility_timeout.dimmed()
            );
            if let Some(id) = data["id"].as_str() {
                println!("  id: {}", id.dimmed());
            }
        }

        QueueCommands::List => {
            let res = client
                .client
                .get(R::queues::LIST.url(&client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let queues = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if queues.is_empty() {
                println!("No queues.");
            } else {
                println!(
                    "{:<25} {:<10} {:<10} {:<12} {}",
                    "NAME".bold(),
                    "MESSAGES".bold(),
                    "IN FLIGHT".bold(),
                    "DLQ SIZE".bold(),
                    "CREATED".bold()
                );
                println!("{}", "─".repeat(85).dimmed());
                for q in queues {
                    println!(
                        "{:<25} {:<10} {:<10} {:<12} {}",
                        q["name"].as_str().unwrap_or(""),
                        q["message_count"].as_i64().unwrap_or(0),
                        q["in_flight_count"].as_i64().unwrap_or(0),
                        q["dlq_count"].as_i64().unwrap_or(0),
                        q["created_at"].as_str().unwrap_or("")
                    );
                }
            }
        }

        QueueCommands::Describe { name } => {
            let res = client
                .client
                .get(R::queues::GET.url_with(&client.base_url, &[("name", name.as_str())]))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            println!("{}", serde_json::to_string_pretty(json.get("data").unwrap_or(&json))?);
        }

        QueueCommands::Publish { name, payload } => {
            let payload_val: Value = serde_json::from_str(&payload)
                .map_err(|e| anyhow::anyhow!("Invalid --payload JSON: {}", e))?;
            let res = client
                .client
                .post(R::queues::PUBLISH.url_with(&client.base_url, &[("name", name.as_str())]))
                .json(&serde_json::json!({ "payload": payload_val }))
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let msg_id = data["message_id"].as_str().unwrap_or("?");
                println!(
                    "{} Published (message_id: {})",
                    "✔".green().bold(),
                    msg_id.dimmed()
                );
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Publish failed: {} — {}", status, body);
            }
        }

        QueueCommands::Bind { name, function } => {
            let res = client
                .client
                .post(R::queues::BINDINGS_LIST.url_with(&client.base_url, &[("name", name.as_str())]))
                .json(&serde_json::json!({ "function_name": function }))
                .send()
                .await?;
            res.error_for_status()?;
            println!(
                "{} Bound queue '{}' → function '{}'",
                "✔".green().bold(),
                name.cyan(),
                function.bold()
            );
        }

        QueueCommands::Bindings { name } => {
            let res = client
                .client
                .get(R::queues::BINDINGS_LIST.url_with(&client.base_url, &[("name", name.as_str())]))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let bindings = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if bindings.is_empty() {
                println!("No bindings for queue '{}'.", name);
            } else {
                println!("{:<40} {}", "FUNCTION".bold(), "CREATED".bold());
                for b in bindings {
                    println!(
                        "{:<40} {}",
                        b["function_name"].as_str().unwrap_or(""),
                        b["created_at"].as_str().unwrap_or("")
                    );
                }
            }
        }

        QueueCommands::Purge { name, confirm } => {
            if !confirm {
                print!("Purge all messages from queue '{}'? [y/N]: ", name.red());
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
                .post(R::queues::PURGE.url_with(&client.base_url, &[("name", name.as_str())]))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Purged queue '{}'", "✔".green().bold(), name.cyan());
        }

        QueueCommands::Delete { name, confirm } => {
            if !confirm {
                print!("Delete queue '{}' and all its messages? [y/N]: ", name.red());
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
                .delete(R::queues::GET.url_with(&client.base_url, &[("name", name.as_str())]))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted queue '{}'", "✔".green().bold(), name);
        }

        QueueCommands::Dlq { command } => match command {
            DlqCommands::List { name } => {
                let res = client
                    .client
                    .get(R::queues::DLQ_LIST.url_with(&client.base_url, &[("name", name.as_str())]))
                    .send()
                    .await?;
                let json: Value = res.error_for_status()?.json().await?;
                let messages = json
                    .get("data")
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default();

                if messages.is_empty() {
                    println!("DLQ is empty for queue '{}'.", name);
                } else {
                    println!(
                        "{:<40} {:<10} {:<25} {}",
                        "MESSAGE ID".bold(),
                        "ATTEMPTS".bold(),
                        "LAST ERROR".bold(),
                        "LAST ATTEMPT".bold()
                    );
                    println!("{}", "─".repeat(90).dimmed());
                    for m in messages {
                        println!(
                            "{:<40} {:<10} {:<25} {}",
                            m["message_id"].as_str().unwrap_or(""),
                            m["attempt_count"].as_i64().unwrap_or(0),
                            m["last_error"].as_str().unwrap_or(""),
                            m["last_attempt_at"].as_str().unwrap_or("")
                        );
                    }
                }
            }
            DlqCommands::Replay { name } => {
                let res = client
                    .client
                    .post(R::queues::DLQ_REPLAY.url_with(&client.base_url, &[("name", name.as_str())]))
                    .send()
                    .await?;
                if res.status().is_success() {
                    let json: Value = res.json().await.unwrap_or_default();
                    let data = json.get("data").unwrap_or(&json);
                    let count = data["replayed"].as_i64().unwrap_or(0);
                    println!("{} Replayed {} DLQ messages for queue '{}'", "✔".green().bold(), count, name.cyan());
                } else {
                    let status = res.status();
                    anyhow::bail!("DLQ replay failed: {}", status);
                }
            }
        },
    }

    Ok(())
}
