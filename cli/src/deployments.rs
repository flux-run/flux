use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;
use crate::project::load_project_config;

#[derive(Debug, Args)]
pub struct DeploymentsArgs {}

pub async fn execute(_args: DeploymentsArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    
    // Resolve project root by looking for flux.json
    let project_dir = if load_project_config(&cwd).is_ok() {
        cwd.clone()
    } else {
        // Try to find the root
        let mut curr = cwd.clone();
        let mut found = None;
        while let Some(parent) = curr.parent() {
            if curr.join("flux.json").exists() {
                found = Some(curr.clone());
                break;
            }
            curr = parent.to_path_buf();
        }
        found.ok_or_else(|| anyhow::anyhow!("No flux.json found in current or parent directories. Are you in a Flux project?"))?
    };

    let history_path = project_dir.join(".flux").join("deployments.json");
    if !history_path.exists() {
        println!("\n  No deployments found for this project.\n  Run `flux build` to create your first build.\n");
        return Ok(());
    }

    let source = std::fs::read_to_string(&history_path)?;
    let history: shared::project::BuildHistory = serde_json::from_str(&source)
        .context("failed to parse deployments.json")?;

    println!("\n  \x1b[1mDeployment History\x1b[0m\n");
    
    if history.deployments.is_empty() {
        println!("  (Empty history)");
    } else {
        println!("  {:^8}  {:^20}  {:^30}", "VERSION", "TIMESTAMP", "ENTRY POINT");
        println!("  {:─^8}  {:─^20}  {:─^30}", "", "", "");

        for dep in history.deployments.iter().rev() {
            let short_sha = &dep.id[..8];
            println!(
                "  \x1b[32m{}\x1b[0m  {:^20}  {}",
                short_sha,
                dep.timestamp,
                dep.entry
            );
        }
    }

    println!(
        "\n  \x1b[2mUse `flux start --version <sha>` to run any historical build.\x1b[0m\n"
    );

    Ok(())
}
