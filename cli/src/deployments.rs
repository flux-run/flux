use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::context::resolve_context;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum DeploymentCommands {
    /// List deployments for a single function
    List {
        name: String,
    },
    /// List project-level deployment history
    History {
        #[arg(long)]
        context: Option<String>,
    },
    /// Rollback the project to a previous deployment version
    Rollback {
        /// Target version number to roll back to
        version: u32,
        #[arg(long)]
        context: Option<String>,
    },
}

pub async fn execute_deployments(command: DeploymentCommands) -> anyhow::Result<()> {
    match command {
        DeploymentCommands::List { name } => {
            let client = crate::client::ApiClient::new().await?;
            let res = client
                .client
                .get(R::deployments::LIST.url_with(&client.base_url, &[("id", name.as_str())]))
                .send()
                .await?;

            let json: Value = res.error_for_status()?.json().await?;
            let deployments = json
                .get("data")
                .and_then(|d| d.get("deployments"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            println!("{:<36} {:<10} {:<15} {}", "ID", "VERSION", "STATUS", "CREATED_AT");
            println!("{}", "-".repeat(85));
            for dep in deployments {
                let id         = dep.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let version    = dep.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
                let is_active  = dep.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
                let status     = dep.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let created    = dep.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                let active_marker = if is_active { "(Active)" } else { "" };
                let version_str   = format!("v{} {}", version, active_marker);
                println!("{:<36} {:<10} {:<15} {}", id, version_str, status, created);
            }
        }
        DeploymentCommands::History { context } => execute_list(context).await?,
        DeploymentCommands::Rollback { version, context } => execute_rollback_version(version, context).await?,
    }
    Ok(())
}

// ── Project-level list ────────────────────────────────────────────────────────

pub async fn execute_list(context_name: Option<String>) -> anyhow::Result<()> {
    let project_root = crate::dev::find_project_root_pub();
    let ctx = resolve_context(context_name.as_deref(), project_root.as_deref())?;

    let client = reqwest::Client::new();
    let url = format!("{}/api/deployments/project", ctx.endpoint);
    let mut req = client.get(&url);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?.error_for_status()?;
    let json: Value = resp.json().await?;
    let deployments = json
        .get("data")
        .and_then(|d| d.get("deployments"))
        .or_else(|| json.get("deployments"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if deployments.is_empty() {
        println!("\n  No project deployments found.\n");
        return Ok(());
    }

    println!();
    println!(
        "  {:<4} {:<9} {:<10} {:<20} {}",
        "#".bold(),
        "Version".bold(),
        "Functions".bold(),
        "Deployed by".bold(),
        "When".bold()
    );
    println!("  {}", "─".repeat(60));

    for (i, dep) in deployments.iter().enumerate() {
        let version     = dep.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
        let deployed_by = dep.get("deployed_by").and_then(|v| v.as_str()).unwrap_or("unknown");
        let created_at  = dep.get("created_at").and_then(|v| v.as_str()).unwrap_or("");

        let fn_count = dep
            .get("summary")
            .and_then(|s| s.get("total"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let when = humanise_time(created_at);

        println!(
            "  {:<4} {:<9} {:<10} {:<20} {}",
            (i + 1).to_string().dimmed(),
            format!("v{}", version).cyan().bold(),
            format!("{} fns", fn_count),
            deployed_by,
            when.dimmed(),
        );
    }
    println!();
    Ok(())
}

// ── Project-level rollback ────────────────────────────────────────────────────

pub async fn execute_rollback_version(
    version:      u32,
    context_name: Option<String>,
) -> anyhow::Result<()> {
    let project_root = crate::dev::find_project_root_pub();
    let ctx = resolve_context(context_name.as_deref(), project_root.as_deref())?;

    let client = reqwest::Client::new();

    // Fetch the project deployment list to find the ID for this version.
    let url = format!("{}/api/deployments/project", ctx.endpoint);
    let mut req = client.get(&url);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?.error_for_status()?;
    let json: Value = resp.json().await?;
    let deployments = json
        .get("data")
        .and_then(|d| d.get("deployments"))
        .or_else(|| json.get("deployments"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let target = deployments
        .iter()
        .find(|d| d.get("version").and_then(|v| v.as_u64()) == Some(version as u64))
        .ok_or_else(|| anyhow::anyhow!("No project deployment with version v{} found", version))?;

    let id = target
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Deployment record has no id"))?;

    // Execute rollback.
    let rollback_url = format!("{}/api/deployments/project/{}/rollback", ctx.endpoint, id);
    let mut roll_req = client.post(&rollback_url);
    if !ctx.api_key.is_empty() {
        roll_req = roll_req.bearer_auth(&ctx.api_key);
    }

    let roll_resp = roll_req.send().await?.error_for_status()?;
    let roll_json: Value = roll_resp.json().await?;
    let data = roll_json.get("data").unwrap_or(&roll_json);

    let rolled_back_to    = data.get("rolled_back_to").and_then(|v| v.as_i64()).unwrap_or(version as i64);
    let functions_restored = data.get("functions_restored").and_then(|v| v.as_i64()).unwrap_or(0);

    println!();
    println!(
        "  {} Rolled back to project v{}",
        "✔".green().bold(),
        rolled_back_to.to_string().bold(),
    );
    println!("     {} function(s) restored", functions_restored);
    println!();
    Ok(())
}

// ── Time humaniser ────────────────────────────────────────────────────────────

fn humanise_time(rfc3339: &str) -> String {
    // Very lightweight — just show the raw timestamp if we can't parse it.
    if rfc3339.is_empty() {
        return "unknown".into();
    }
    // Strip sub-second precision and timezone for brevity.
    if let Some(t) = rfc3339.get(..19) {
        return t.replace('T', " ");
    }
    rfc3339.to_string()
}
