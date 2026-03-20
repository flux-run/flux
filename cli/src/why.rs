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

    // Header: method + path + status
    let status_display = match analysis.status.as_str() {
        "ok" => "\x1b[32m✓ ok\x1b[0m",
        "error" => "\x1b[31m✗ error\x1b[0m",
        _ => &analysis.status,
    };
    let short_id = &analysis.execution_id[..analysis.execution_id.len().min(8)];
    println!(
        "  \x1b[1m{} {}\x1b[0m  {}  {}ms  \x1b[2m{}\x1b[0m",
        analysis.method, analysis.path, status_display, analysis.duration_ms, short_id
    );
    println!();

    // Reason block
    for line in analysis.reason.lines() {
        let (label, rest) = if let Some(idx) = line.find("   ") {
            (&line[..idx + 3], &line[idx + 3..])
        } else {
            ("", line)
        };
        if label.trim().is_empty() {
            // Section header / plain line
            if analysis.status == "error" {
                println!("  \x1b[31m{}\x1b[0m", line);
            } else {
                println!("  \x1b[33m{}\x1b[0m", line);
            }
        } else {
            println!("  \x1b[2m{}\x1b[0m{}", label, rest);
        }
    }

    // Actual error body (decoded from response)
    if !analysis.error_body.is_empty() {
        println!();
        println!("  \x1b[2merror body\x1b[0m");
        for line in analysis.error_body.lines() {
            println!("    \x1b[31m{}\x1b[0m", line);
        }
    }

    // Console logs captured during the execution
    if !analysis.logs.is_empty() {
        println!();
        println!("  \x1b[2mconsole\x1b[0m");
        for (level, message) in &analysis.logs {
            let color = match level.as_str() {
                "error" => "\x1b[31m",
                "warn" => "\x1b[33m",
                _ => "\x1b[0m",
            };
            let icon = match level.as_str() {
                "error" => "✗",
                "warn" => "⚠",
                _ => "›",
            };
            println!("    {}{} {}\x1b[0m", color, icon, message);
        }
    }

    // Suggestion footer
    if !analysis.suggestion.trim().is_empty() {
        println!();
        println!("  \x1b[2m{}\x1b[0m", analysis.suggestion.trim());
    }

    println!();
    Ok(())
}
