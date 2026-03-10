//! `flux version` — manage function deployment versions.
//!
//! Note: this is the *deployment versioning* namespace.
//! Use `flux --version` to query the CLI version itself.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum VersionCommands {
    /// List all deployment versions for a function
    List {
        /// Function name
        function: String,
    },
    /// Get details of a specific version
    Get {
        /// Function name
        function: String,
        #[arg(long)]
        version: i32,
    },
    /// Roll back a function to a previous version
    Rollback {
        /// Function name
        function: String,
        /// Target version number
        #[arg(long)]
        to: i32,
    },
    /// Promote a version to a different environment
    Promote {
        /// Function name
        function: String,
        #[arg(long)]
        version: i32,
        /// Target environment (e.g. production, staging)
        #[arg(long)]
        to: String,
    },
    /// Show diff between two versions (bundle metadata changes)
    Diff {
        /// Function name
        function: String,
        #[arg(long)]
        from: i32,
        #[arg(long)]
        to: i32,
    },
}

pub async fn execute(command: VersionCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        VersionCommands::List { function } => {
            let res = client
                .client
                .get(format!("{}/functions/{}/deployments", client.base_url, function))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let deps = json
                .get("data")
                .and_then(|d| d.get("deployments"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            println!(
                "{:<38} {:<10} {:<15} {}",
                "ID".bold(),
                "VERSION".bold(),
                "STATUS".bold(),
                "CREATED_AT".bold()
            );
            println!("{}", "─".repeat(85).dimmed());
            for dep in deps {
                let id = dep["id"].as_str().unwrap_or("");
                let version = dep["version"].as_i64().unwrap_or(0);
                let is_active = dep["is_active"].as_bool().unwrap_or(false);
                let status = dep["status"].as_str().unwrap_or("");
                let created = dep["created_at"].as_str().unwrap_or("");
                let active_tag = if is_active { " (active)" } else { "" };
                let version_str = format!("v{}{}", version, active_tag);
                let version_col = if is_active {
                    version_str.green().bold().to_string()
                } else {
                    version_str.normal().to_string()
                };
                println!("{:<38} {:<10} {:<15} {}", id, version_col, status, created);
            }
        }

        VersionCommands::Get { function, version } => {
            let res = client
                .client
                .get(format!(
                    "{}/functions/{}/deployments/{}",
                    client.base_url, function, version
                ))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            println!("{}", serde_json::to_string_pretty(json.get("data").unwrap_or(&json))?);
        }

        VersionCommands::Rollback { function, to } => {
            let res = client
                .client
                .post(format!(
                    "{}/functions/{}/deployments/{}/activate",
                    client.base_url, function, to
                ))
                .send()
                .await?;
            res.error_for_status()?;
            println!(
                "{} Rolled back {} to v{}",
                "✔".green().bold(),
                function.cyan(),
                to
            );
        }

        VersionCommands::Promote { function, version, to } => {
            let body = serde_json::json!({
                "version": version,
                "target_env": to,
            });
            let res = client
                .client
                .post(format!(
                    "{}/functions/{}/deployments/{}/promote",
                    client.base_url, function, version
                ))
                .json(&body)
                .send()
                .await?;
            res.error_for_status()?;
            println!(
                "{} Promoted {} v{} to {}",
                "✔".green().bold(),
                function.cyan(),
                version,
                to.bold()
            );
        }

        VersionCommands::Diff { function, from, to } => {
            let res = client
                .client
                .get(format!(
                    "{}/functions/{}/deployments/diff?from={}&to={}",
                    client.base_url, function, from, to
                ))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);

            println!(
                "\n  Diff: {} v{} → v{}\n",
                function.cyan(),
                from,
                to
            );

            if let Some(changes) = data["changes"].as_array() {
                for change in changes {
                    let field = change["field"].as_str().unwrap_or("");
                    let old_val = change["old"].as_str().unwrap_or("(none)");
                    let new_val = change["new"].as_str().unwrap_or("(none)");
                    println!(
                        "  {}  {}: {} → {}",
                        "~".yellow(),
                        field.bold(),
                        old_val.red(),
                        new_val.green()
                    );
                }
            } else {
                println!("  No schema changes between v{} and v{}.", from, to);
            }
            println!();
        }
    }

    Ok(())
}
