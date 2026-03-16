use anyhow::{Context, Result};
use std::io::Write;

use crate::config::CliConfig;
use crate::grpc::{normalize_grpc_url, validate_service_token};

pub async fn execute() -> Result<()> {
    println!("Flux init\n");

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

    let token = rpassword::prompt_password("Service token: ")
        .context("failed to read service token")?;

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
    println!("\ntry:");
    println!("  flux logs");

    Ok(())
}
