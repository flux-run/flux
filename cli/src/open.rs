//! `flux open` — open the local trace viewer in a browser.
//!
//! In the self-hosted framework there is no cloud dashboard.
//! The trace viewer is served by the gateway — its URL comes from
//! `flux.toml [dev]`, `FLUX_GATEWAY_URL`, or the default `http://localhost:4000`.

use clap::Subcommand;
use colored::Colorize;

use crate::config::Config;

#[derive(Subcommand)]
pub enum OpenCommands {
    /// Open the trace viewer for a specific request ID
    Trace {
        /// Request / trace ID
        id: String,
    },
    /// Open the local gateway root
    Gateway,
}

pub async fn execute(command: OpenCommands) -> anyhow::Result<()> {
    let config = Config::load().await;
    let url = build_url(&command, &config.gateway_url);
    println!("Opening: {}", url.cyan());
    open::that(&url).map_err(|e| anyhow::anyhow!("Failed to open browser: {}", e))?;
    Ok(())
}

pub async fn execute_default() -> anyhow::Result<()> {
    let config = Config::load().await;
    let url = config.gateway_url.trim_end_matches('/').to_string();
    println!("Opening: {}", url.cyan());
    open::that(&url).map_err(|e| anyhow::anyhow!("Failed to open browser: {}", e))?;
    Ok(())
}

fn build_url(command: &OpenCommands, gateway_url: &str) -> String {
    let base = gateway_url.trim_end_matches('/');
    match command {
        OpenCommands::Trace { id } => format!("{}/trace/{}", base, id),
        OpenCommands::Gateway     => base.to_string(),
    }
}
