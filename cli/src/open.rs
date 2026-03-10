//! `flux open` — open the Fluxbase dashboard or a specific resource in the browser.

use clap::Subcommand;
use colored::Colorize;

use crate::config::Config;

const DASHBOARD_BASE: &str = "https://app.fluxbase.co";

#[derive(Subcommand)]
pub enum OpenCommands {
    /// Open the project dashboard (default)
    Dashboard,
    /// Open the function detail page
    Function {
        /// Function name
        name: String,
    },
    /// Open trace detail in the dashboard
    Trace {
        /// Request/trace ID
        id: String,
    },
    /// Open the live log stream in the dashboard
    Logs,
    /// Open the gateway routes editor
    Gateway,
    /// Open the secrets manager page
    Secrets,
    /// Open the API keys management page
    ApiKeys,
}

pub async fn execute(command: OpenCommands) -> anyhow::Result<()> {
    let config = Config::load().await;
    let tenant = config.tenant_slug.clone().unwrap_or_default();
    let project = config.project_id.clone().unwrap_or_default();
    let url = build_url(&command, &tenant, &project);
    println!("Opening: {}", url.cyan());
    open::that(&url).map_err(|e| anyhow::anyhow!("Failed to open browser: {}", e))?;
    Ok(())
}

/// Also called when `flux open` is invoked with no sub-command (defaults to dashboard).
pub async fn execute_default() -> anyhow::Result<()> {
    execute(OpenCommands::Dashboard).await
}

fn build_url(command: &OpenCommands, tenant: &str, project: &str) -> String {
    let base = if !tenant.is_empty() && !project.is_empty() {
        format!("{}/{}/{}", DASHBOARD_BASE, tenant, project)
    } else if !tenant.is_empty() {
        format!("{}/{}", DASHBOARD_BASE, tenant)
    } else {
        DASHBOARD_BASE.to_string()
    };

    match command {
        OpenCommands::Dashboard => base,
        OpenCommands::Function { name } => format!("{}/functions/{}", base, name),
        OpenCommands::Trace { id } => format!("{}/traces/{}", base, id),
        OpenCommands::Logs => format!("{}/logs", base),
        OpenCommands::Gateway => format!("{}/gateway", base),
        OpenCommands::Secrets => format!("{}/secrets", base),
        OpenCommands::ApiKeys => format!("{}/api-keys", base),
    }
}
