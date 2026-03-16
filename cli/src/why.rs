use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::why;

#[derive(Debug, Args)]
pub struct WhyArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
}

pub async fn execute(args: WhyArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let analysis = why(&auth.url, &auth.token, &args.execution_id).await?;

    println!();
    for line in analysis.reason.lines() {
        println!("  {}", line);
    }

    if !analysis.suggestion.trim().is_empty() {
        println!();
        println!("  {}", analysis.suggestion.trim());
    }

    println!();
    Ok(())
}
