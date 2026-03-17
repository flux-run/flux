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

    if !trace.logs.is_empty() {
        println!();
        println!("  console logs");
        for log in &trace.logs {
            println!("  [{}] {}", log.level, log.message);
        }
    }

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

        if cp.boundary == "timer" {
            let requested_delay_ms = req
                .get("requested_delay_ms")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let effective_delay_ms = res
                .get("effective_delay_ms")
                .and_then(|value| value.as_f64())
                .unwrap_or(requested_delay_ms);

            println!(
                "  [{}] TIMER  requested={}ms  effective={}ms",
                cp.call_index,
                requested_delay_ms,
                effective_delay_ms,
            );

            if args.verbose {
                let request_json = serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json = serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

                println!("      request");
                print_json_block(&request_json, true);
                println!("      response");
                print_json_block(&response_json, true);
            }
            continue;
        }

        if cp.boundary == "tcp" {
            let host = req
                .get("host")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let port = req
                .get("port")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let tls = req
                .get("tls")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let bytes_read = res
                .get("bytes_read")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);

            println!(
                "  [{}] TCP{}  {}:{}  {}ms  → {} bytes",
                cp.call_index,
                if tls { "+TLS" } else { "" },
                host,
                port,
                cp.duration_ms,
                bytes_read,
            );

            if args.verbose {
                let request_json = serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json = serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

                println!("      request");
                print_json_block(&request_json, true);
                println!("      response");
                print_json_block(&response_json, true);
            }
            continue;
        }

        if cp.boundary == "postgres" {
            let host = req
                .get("host")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let port = req
                .get("port")
                .and_then(|value| value.as_u64())
                .unwrap_or(5432);
            let sql = req
                .get("sql")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let row_count = res
                .get("row_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);

            println!(
                "  [{}] POSTGRES  {}:{}  {}ms  → {} rows  {}",
                cp.call_index,
                host,
                port,
                cp.duration_ms,
                row_count,
                sql,
            );

            if args.verbose {
                let request_json = serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json = serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

                println!("      request");
                print_json_block(&request_json, true);
                println!("      response");
                print_json_block(&response_json, true);
            }
            continue;
        }

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
