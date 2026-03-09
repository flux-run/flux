//! `flux init` — create `.fluxbase/config.json` for this project directory.
//!
//! ```text
//! $ flux init --project proj_abc123 --output src/fluxbase.generated.ts
//! ✔ Created .fluxbase/config.json
//!
//! Contents:
//!   project_id:     proj_abc123
//!   sdk_output:     src/fluxbase.generated.ts
//!   watch_interval: 5
//!
//! Commit this file to share settings with your team.
//! Run: flux pull
//! ```

use colored::Colorize;

use crate::config::{Config, ProjectConfig};

pub async fn execute(
    project:     Option<String>,
    output:      Option<String>,
    interval:    Option<u64>,
    api_url:     Option<String>,
    gateway_url: Option<String>,
) -> anyhow::Result<()> {
    // Fallback: read project_id from global config if not supplied as a flag.
    let project_id = match project {
        Some(p) => Some(p),
        None => {
            let global = Config::load().await;
            global.project_id
        }
    };

    let proj = ProjectConfig {
        project_id:     project_id.clone(),
        sdk_output:     output.clone(),
        watch_interval: interval,
        api_url:        api_url.clone(),
        gateway_url:    gateway_url.clone(),
    };

    let path = proj.save().await?;

    println!("{} Created {}", "✔".green().bold(), path.display().to_string().cyan().bold());
    println!();

    // Echo what was written.
    if let Some(pid) = &project_id {
        println!("  {}  {}", "project_id:    ".bold(), pid.cyan());
    }
    let sdk_out = output.as_deref().unwrap_or("fluxbase.generated.ts");
    println!("  {}  {}", "sdk_output:    ".bold(), sdk_out.cyan());
    println!(
        "  {}  {}",
        "watch_interval:".bold(),
        interval.unwrap_or(5).to_string().cyan()
    );
    if let Some(u) = &api_url {
        println!("  {}  {}", "api_url:       ".bold(), u.cyan());
    }
    if let Some(u) = &gateway_url {
        println!("  {}  {}", "gateway_url:   ".bold(), u.cyan());
    }
    println!();
    println!("{}", "Commit .fluxbase/config.json to share settings with your team.".dimmed());
    println!("Run: {}", "flux pull".cyan().bold());

    Ok(())
}
