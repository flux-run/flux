//! `flux monitor` — platform observability: status, metrics, and alerts.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum MonitorCommands {
    /// Show health status of all platform services
    Status,
    /// Show metrics for a function
    Metrics {
        /// Function name
        #[arg(long)]
        function: Option<String>,
        /// Time window (e.g. 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        window: String,
    },
    /// Manage alerts
    Alerts {
        #[command(subcommand)]
        command: AlertCommands,
    },
}

#[derive(Subcommand)]
pub enum AlertCommands {
    /// Create a new alert
    Create {
        #[arg(long)]
        name: String,
        /// Metric to monitor (e.g. function_error_rate)
        #[arg(long)]
        metric: String,
        /// Function to watch
        #[arg(long)]
        function: Option<String>,
        /// Threshold value (e.g. 0.05 for 5%)
        #[arg(long)]
        threshold: f64,
        /// Evaluation window (e.g. 5m, 1h)
        #[arg(long, default_value = "5m")]
        window: String,
        /// Notification channel (email | slack | webhook)
        #[arg(long, default_value = "email")]
        notify: String,
    },
    /// List all alerts
    List,
    /// Delete an alert
    Delete {
        id: String,
    },
}

pub async fn execute(command: MonitorCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        MonitorCommands::Status => {
            let res = client
                .client
                .get(R::monitor::STATUS.url(&client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let services = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            println!(
                "\n  {:<22} {:<10} {:<22} {}",
                "SERVICE".bold(),
                "STATUS".bold(),
                "LATENCY (p50/p95)".bold(),
                "ERROR RATE (1h)".bold()
            );
            println!("  {}", "─".repeat(75).dimmed());

            for svc in services {
                let name = svc["service"].as_str().unwrap_or("");
                let status = svc["status"].as_str().unwrap_or("unknown");
                let p50 = svc["p50_ms"].as_i64().unwrap_or(0);
                let p95 = svc["p95_ms"].as_i64().unwrap_or(0);
                let err_rate = svc["error_rate_1h"].as_f64().unwrap_or(0.0);

                let status_col = match status {
                    "healthy" => status.green().bold(),
                    "degraded" => status.yellow().bold(),
                    _ => status.red().bold(),
                };

                println!(
                    "  {:<22} {:<10} {:<22} {:.1}%",
                    name,
                    status_col,
                    format!("{}ms / {}ms", p50, p95),
                    err_rate * 100.0
                );
            }
            println!();
        }

        MonitorCommands::Metrics { function, window } => {
            let mut q = vec![("window", window.as_str())];
            if let Some(ref f) = function { q.push(("function", f.as_str())); }
            let json: Value = client.get_with(&R::monitor::METRICS, &[], &q).await?;
            let data = json.get("data").unwrap_or(&json);

            println!();
            if let Some(f) = &function {
                println!("  Metrics for {} (window: {})", f.cyan().bold(), window);
            } else {
                println!("  Platform metrics (window: {})", window);
            }
            println!("  {}", "─".repeat(40).dimmed());

            let invocations = data["invocations"].as_i64().unwrap_or(0);
            let success_rate = data["success_rate"].as_f64().unwrap_or(1.0) * 100.0;
            let p50 = data["p50_duration_ms"].as_i64().unwrap_or(0);
            let p95 = data["p95_duration_ms"].as_i64().unwrap_or(0);
            let errors = data["errors"].as_i64().unwrap_or(0);

            println!("  {:<20} {}", "invocations:".bold(), invocations);
            println!("  {:<20} {:.1}%", "success_rate:".bold(), success_rate);
            println!("  {:<20} {}ms", "p50_duration:".bold(), p50);
            println!("  {:<20} {}ms", "p95_duration:".bold(), p95);
            println!("  {:<20} {}", "errors:".bold(), errors);
            println!();
        }

        MonitorCommands::Alerts { command } => alerts_cmd(command, &client).await?,
    }

    Ok(())
}

async fn alerts_cmd(command: AlertCommands, client: &ApiClient) -> anyhow::Result<()> {
    match command {
        AlertCommands::Create { name, metric, function, threshold, window, notify } => {
            let body = serde_json::json!({
                "name": name,
                "metric": metric,
                "function_name": function,
                "threshold": threshold,
                "window": window,
                "notify": notify,
            });
            let res = client
                .client
                .post(R::monitor::ALERTS_LIST.url(&client.base_url))
                .json(&body)
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!(
                "{} Alert '{}' created (id: {})",
                "✔".green().bold(),
                name.cyan(),
                data["id"].as_str().unwrap_or("?").dimmed()
            );
        }

        AlertCommands::List => {
            let res = client
                .client
                .get(R::monitor::ALERTS_LIST.url(&client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let alerts = json
                .get("data")
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();

            if alerts.is_empty() {
                println!("No alerts configured.");
            } else {
                println!(
                    "{:<38} {:<25} {:<15} {:<10} {}",
                    "ID".bold(),
                    "NAME".bold(),
                    "METRIC".bold(),
                    "THRESHOLD".bold(),
                    "NOTIFY".bold()
                );
                println!("{}", "─".repeat(95).dimmed());
                for a in alerts {
                    println!(
                        "{:<38} {:<25} {:<15} {:<10} {}",
                        a["id"].as_str().unwrap_or(""),
                        a["name"].as_str().unwrap_or(""),
                        a["metric"].as_str().unwrap_or(""),
                        a["threshold"].as_f64().unwrap_or(0.0),
                        a["notify"].as_str().unwrap_or("")
                    );
                }
            }
        }

        AlertCommands::Delete { id } => {
            let res = client
                .client
                .delete(R::monitor::ALERTS_DELETE.url_with(&client.base_url, &[("id", &id)]))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted alert {}", "✔".green().bold(), id.dimmed());
        }
    }
    Ok(())
}
