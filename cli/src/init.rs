use anyhow::{Context, Result};
use clap::Args;
use std::io::Write;

use crate::config::CliConfig;
use crate::grpc::{normalize_grpc_url, validate_service_token};
use crate::project::scaffold_project;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long)]
    pub auth: bool,

    #[arg(long)]
    pub force: bool,
}

pub async fn execute(args: InitArgs) -> Result<()> {
    if args.auth {
        return init_auth().await;
    }

    let cwd = std::env::current_dir().context("failed to read current directory")?;
    scaffold_project(&cwd, args.force)?;

    println!("\n  ✔  Project initialized successfully\n");
    println!("  Created:");
    println!("    - ./flux.json");
    println!("    - ./src/index.ts\n");

    let config = CliConfig::load().unwrap_or_default();
    if config.token.is_some() {
        println!("  Authentication:");
        println!("    ✔  Logged in to {}", config.url.unwrap_or_default());
    } else {
        println!("  Next steps:");
        println!("    1. flux login  (to connect to Flux Cloud)");
    }

    println!("\n  Development:");
    println!("    - flux dev     (start the local development server)");

    Ok(())
}

async fn init_auth() -> Result<()> {
    println!("Flux auth init\n");

    let mut url = String::new();
    print!("Server URL (default: localhost:50051): ");
    std::io::stdout().flush().ok();
    std::io::stdin()
        .read_line(&mut url)
        .context("failed to read server URL")?;
    let url = {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            "localhost:50051".to_string()
        } else {
            trimmed.to_string()
        }
    };

    let token =
        rpassword::prompt_password("Service token: ").context("failed to read service token")?;

    let normalized_url = normalize_grpc_url(&url);
    let auth_mode = validate_service_token(&normalized_url, &token).await?;

    let config = CliConfig {
        url: Some(normalized_url.clone()),
        token: Some(token),
    };
    config.save()?;

    println!("\n✓ saved config to ~/.flux/config.toml");
    println!("  server: {}", normalized_url);
    println!("  auth:   {}", auth_mode);

    Ok(())
}
