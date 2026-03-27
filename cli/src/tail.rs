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
        let status_display = match event.status.as_str() {
            "ok" => "\x1b[32m✓  ok\x1b[0m",
            "error" => "\x1b[31m✗  error\x1b[0m",
            "running" => "\x1b[33m…  running\x1b[0m",
            _ => "?",
        };

        // Show only first 8 chars of execution_id like `flux logs` does
        let short_id = &event.execution_id[..event.execution_id.len().min(8)];

        if event.error.is_empty() {
            println!(
                "  {}  \x1b[1m{}\x1b[0m {}  {}ms  \x1b[2m{}\x1b[0m",
                status_display, event.method, event.path, event.duration_ms, short_id
            );
        } else {
            println!(
                "  {}  \x1b[1m{}\x1b[0m {}  {}ms  \x1b[2m{}\x1b[0m\n     \x1b[31m└─ {}\x1b[0m",
                status_display, event.method, event.path, event.duration_ms, short_id, event.error
            );
        }
    }

    Ok(())
}
