use anyhow::{Context, Result};
use clap::Args;

use crate::config::CliConfig;
use crate::grpc::{normalize_grpc_url, validate_service_token};

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[arg(long, value_name = "URL")]
    pub url: String,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub skip_verify: bool,
}

pub async fn execute(args: AuthArgs) -> Result<()> {
    let token = match args.token {
        Some(token) => token,
        None => {
            rpassword::prompt_password("Service token: ").context("failed to read service token")?
        }
    };

    let url = normalize_grpc_url(&args.url);
    if !args.skip_verify {
        let auth_mode = validate_service_token(&url, &token).await?;
        println!("authenticated against {} using {} auth", url, auth_mode);
    }

    let config = CliConfig {
        url: Some(url.clone()),
        token: Some(token),
    };
    config.save()?;

    println!("saved CLI auth config");
    println!("server:  {}", url);

    Ok(())
}
