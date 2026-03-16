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
    #[arg(long)]
    pub verbose: bool,
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

    println!();
    println!("  request");
    print_json_block(&trace.request_json, args.verbose);

    println!();
    println!("  response");
    print_json_block(&trace.response_json, args.verbose);

    if trace.checkpoints.is_empty() {
        println!("\n  no checkpoints recorded\n");
        return Ok(());
    }

    println!();
    println!("  checkpoints");
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

        if args.verbose {
            let request_json = serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
            let response_json = serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

            println!("      request");
            print_json_block(&request_json, true);
            println!("      response");
            print_json_block(&response_json, true);
        }
    }

    println!();
    Ok(())
}

fn print_json_block(raw: &str, expanded: bool) {
    if !expanded {
        println!("    (hidden, use --verbose)");
        return;
    }

    let value = serde_json::from_str::<serde_json::Value>(raw).unwrap_or(serde_json::Value::String(raw.to_string()));
    let formatted = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string());
    for line in formatted.lines() {
        println!("    {}", line);
    }
}
