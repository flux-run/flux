use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::tail;

#[derive(Debug, Args)]
pub struct TailArgs {
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long, value_name = "PROJECT_ID")]
    pub project_id: Option<String>,
}

pub async fn execute(args: TailArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;

    println!();
    println!("  streaming live requests — ctrl+c to stop");
    println!();

    let mut stream = tail(&auth.url, &auth.token, args.project_id).await?;

    while let Some(event) = stream.message().await? {
        let status_symbol = match event.status.as_str() {
            "ok" => "✓",
            "error" => "✗",
            "running" => "…",
            _ => "?",
        };

        if event.error.is_empty() {
            println!(
                "  {}  {} {}  {}ms  {}",
                status_symbol, event.method, event.path, event.duration_ms, event.execution_id
            );
        } else {
            println!(
                "  {}  {} {}  {}ms  {}\n     └─ {}",
                status_symbol,
                event.method,
                event.path,
                event.duration_ms,
                event.execution_id,
                event.error
            );
        }
    }

    Ok(())
}
