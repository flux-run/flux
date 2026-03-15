//! `flux event` — pub/sub event bus. Every subscriber receives a copy.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum EventCommands {
    /// Publish an event to all subscribers
    Publish {
        /// Event type (e.g. user.signed_up)
        event_type: String,
        /// JSON payload
        #[arg(long, value_name = "JSON")]
        payload: String,
    },
    /// Subscribe a function to an event type
    Subscribe {
        /// Event type
        event_type: String,
        /// Function name to invoke on each event
        #[arg(long)]
        function: String,
    },
    /// Remove a subscription
    Unsubscribe {
        /// Subscription ID
        subscription_id: String,
    },
    /// List all event subscriptions
    List,
    /// Show recent event history for an event type
    History {
        /// Event type
        event_type: String,
        /// Time window (e.g. 1h, 30m, 24h)
        #[arg(long, default_value = "1h")]
        since: String,
    },
}

pub async fn execute(command: EventCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        EventCommands::Publish { event_type, payload } => {
            let payload_val: Value = serde_json::from_str(&payload)
                .map_err(|e| anyhow::anyhow!("Invalid --payload JSON: {}", e))?;
            let res = client
                .client
                .post(R::events::PUBLISH.url(&client.base_url))
                .json(&serde_json::json!({
                    "type": event_type,
                    "payload": payload_val,
                }))
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let event_id = data["event_id"].as_str().unwrap_or("?");
                println!(
                    "{} Published (event_id: {})",
                    "✔".green().bold(),
                    event_id.dimmed()
                );
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Publish failed: {} — {}", status, body);
            }
        }

        EventCommands::Subscribe { event_type, function } => {
            let res = client
                .client
                .post(R::events::SUBSCRIPTIONS_LIST.url(&client.base_url))
                .json(&serde_json::json!({
                    "event_type": event_type,
                    "function_name": function,
                }))
                .send()
                .await?;

            if res.status().is_success() {
                let json: Value = res.json().await.unwrap_or_default();
                let data = json.get("data").unwrap_or(&json);
                let sub_id = data["subscription_id"].as_str().unwrap_or("?");
                println!(
                    "{} Subscribed: {} → {}  (sub_id: {})",
                    "✔".green().bold(),
                    event_type.cyan(),
                    function.bold(),
                    sub_id.dimmed()
                );
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Subscribe failed: {} — {}", status, body);
            }
        }

        EventCommands::Unsubscribe { subscription_id } => {
            let res = client
                .client
                .delete(R::events::SUBSCRIPTIONS_DELETE.url_with(&client.base_url, &[("id", subscription_id.as_str())]))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Unsubscribed {}", "✔".green().bold(), subscription_id.dimmed());
        }

        EventCommands::List => {
            let res = client
                .client
                .get(R::events::SUBSCRIPTIONS_LIST.url(&client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let subs = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if subs.is_empty() {
                println!("No event subscriptions.");
            } else {
                println!(
                    "{:<38} {:<30} {}",
                    "SUBSCRIPTION ID".bold(),
                    "EVENT TYPE".bold(),
                    "FUNCTION".bold()
                );
                println!("{}", "─".repeat(80).dimmed());
                for s in subs {
                    println!(
                        "{:<38} {:<30} {}",
                        s["subscription_id"].as_str().unwrap_or(""),
                        s["event_type"].as_str().unwrap_or(""),
                        s["function_name"].as_str().unwrap_or("")
                    );
                }
            }
        }

        EventCommands::History { event_type, since } => {
            let res = client
                .client
                .get(format!(
                    "{}/events/history?type={}&since={}",
                    client.base_url, event_type, since
                ))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let events = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if events.is_empty() {
                println!("No events of type '{}' in the last {}.", event_type, since);
            } else {
                println!(
                    "{:<40} {:<30} {:<25} {}",
                    "EVENT ID".bold(),
                    "TYPE".bold(),
                    "PUBLISHED AT".bold(),
                    "SUBSCRIBERS TRIGGERED".bold()
                );
                println!("{}", "─".repeat(95).dimmed());
                for e in events {
                    println!(
                        "{:<40} {:<30} {:<25} {}",
                        e["event_id"].as_str().unwrap_or(""),
                        e["type"].as_str().unwrap_or(""),
                        e["published_at"].as_str().unwrap_or(""),
                        e["triggered_count"].as_i64().unwrap_or(0)
                    );
                }
            }
        }
    }

    Ok(())
}
