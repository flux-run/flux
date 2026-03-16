use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::get_trace;

#[derive(Debug, Args)]
pub struct TraceArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
}

pub async fn execute(args: TraceArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let trace = get_trace(&auth.url, &auth.token, &args.execution_id).await?;

    println!();
    println!(
        "  {} {}  {}  {}ms",
        trace.method,
        trace.path,
        trace.status,
        trace.duration_ms
    );

    if !trace.error.is_empty() {
        println!("  error  {}", trace.error);
    }

    if trace.checkpoints.is_empty() {
        println!("\n  no checkpoints recorded\n");
        return Ok(());
    }

    println!();
    for cp in trace.checkpoints {
        let req: serde_json::Value = serde_json::from_slice(&cp.request).unwrap_or_default();
        let res: serde_json::Value = serde_json::from_slice(&cp.response).unwrap_or_default();

        let url = req
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let status = res
            .get("status")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);

        println!(
            "  [{}] {}  {}  {}ms  → {}",
            cp.call_index,
            cp.boundary.to_uppercase(),
            url,
            cp.duration_ms,
            status
        );
    }

    println!();
    Ok(())
}
