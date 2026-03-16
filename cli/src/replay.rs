use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::replay;

#[derive(Debug, Args)]
pub struct ReplayArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long)]
    pub commit: bool,
    #[arg(long, value_name = "INDEX")]
    pub from_index: Option<i32>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
}

pub async fn execute(args: ReplayArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let from_index = args.from_index.unwrap_or(0).max(0);

    let short_id = if args.execution_id.len() >= 8 {
        &args.execution_id[..8]
    } else {
        &args.execution_id
    };

    println!();
    println!("  replaying {}…", short_id);
    println!();

    let response = replay(
        &auth.url,
        &auth.token,
        &args.execution_id,
        args.commit,
        from_index,
    )
    .await?;

    let status_symbol = if response.status == "ok" { "✓" } else { "✗" };
    println!(
        "  {}  {}  {}ms",
        status_symbol, response.status, response.duration_ms
    );

    if !response.error.is_empty() {
        println!("  error  {}", response.error);
    }

    if !response.output.is_empty() && response.output != "null" {
        println!("  output {}", response.output);
    }

    println!();
    for step in &response.steps {
        let source = if step.used_recorded { "recorded" } else { "live" };
        println!(
            "  [{}] {}  {}  {}ms  ({})",
            step.call_index,
            step.boundary.to_uppercase(),
            step.url,
            step.duration_ms,
            source
        );
    }

    if !args.commit {
        println!();
        println!("  db writes suppressed — pass --commit to apply");
    }

    println!();
    Ok(())
}
