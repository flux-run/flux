use anyhow::Result;
use clap::Args;
use chrono::{DateTime, Duration, Utc};

use crate::config::resolve_auth;
use crate::grpc::list_logs;

#[derive(Debug, Args)]
pub struct LogsArgs {
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long, default_value_t = 50)]
    pub limit: u32,
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
    #[arg(long, value_name = "DURATION")]
    pub since: Option<String>,
    #[arg(long, value_name = "TEXT")]
    pub search: Option<String>,
}

pub async fn execute(args: LogsArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let fetch_limit = if args.status.is_some() || args.path.is_some() || args.since.is_some() || args.search.is_some() {
        (args.limit.saturating_mul(10)).clamp(50, 500)
    } else {
        args.limit
    };
    let mut logs = list_logs(&auth.url, &auth.token, fetch_limit).await?;

    if let Some(status_filter) = args.status.as_deref() {
        let status_filter = status_filter.to_ascii_lowercase();
        logs.retain(|row| row.status.eq_ignore_ascii_case(&status_filter));
    }

    if let Some(path_filter) = args.path.as_deref() {
        logs.retain(|row| row.path.contains(path_filter));
    }

    if let Some(since_filter) = args.since.as_deref() {
        if let Some(cutoff) = parse_since_cutoff(since_filter) {
            logs.retain(|row| {
                DateTime::parse_from_rfc3339(&row.timestamp)
                    .map(|ts| ts.with_timezone(&Utc) >= cutoff)
                    .unwrap_or(false)
            });
        }
    }

    if let Some(search) = args.search.as_deref() {
        let search = search.to_ascii_lowercase();
        logs.retain(|row| {
            row.error.to_ascii_lowercase().contains(&search)
                || row.path.to_ascii_lowercase().contains(&search)
                || row.method.to_ascii_lowercase().contains(&search)
                || row.execution_id.to_ascii_lowercase().contains(&search)
                || row.request_id.to_ascii_lowercase().contains(&search)
        });
    }

    logs.truncate(args.limit as usize);

    if logs.is_empty() {
        println!("\n  no executions matched\n");
        return Ok(());
    }

    println!();
    println!("  TIME      METHOD  PATH               STATUS   DURATION  ID");
    for log in logs {
        let short_id: String = log.execution_id.chars().take(8).collect();
        let time = short_time(&log.timestamp);
        let method = pad(&log.method, 6);
        let path = pad(&log.path, 18);
        let duration = format!("{}ms", log.duration_ms);
        let status = status_label(&log.status, log.duration_ms);

        println!(
            "  {}  {}  {}  {}  {:>8}  {}",
            time,
            method,
            path,
            status,
            duration,
            short_id,
        );
    }

    println!();
    println!("  showing last {} — flux logs --limit 100 for more", args.limit);
    println!();

    Ok(())
}

fn pad(value: &str, width: usize) -> String {
    let clipped: String = value.chars().take(width).collect();
    format!("{:<width$}", clipped, width = width)
}

fn short_time(ts: &str) -> String {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(ts) {
        parsed.with_timezone(&Utc).format("%H:%M:%S").to_string()
    } else {
        ts.to_string()
    }
}

fn parse_since_cutoff(input: &str) -> Option<DateTime<Utc>> {
    let value = input.trim().to_ascii_lowercase();
    if value.len() < 2 {
        return None;
    }
    let (n, suffix) = value.split_at(value.len() - 1);
    let amount: i64 = n.parse().ok()?;
    let delta = match suffix {
        "s" => Duration::seconds(amount),
        "m" => Duration::minutes(amount),
        "h" => Duration::hours(amount),
        "d" => Duration::days(amount),
        _ => return None,
    };
    Some(Utc::now() - delta)
}

fn status_label(status: &str, duration_ms: i32) -> String {
    let lower = status.to_ascii_lowercase();
    if lower == "error" {
        return "\x1b[31m✗ error\x1b[0m".to_string();
    }
    if duration_ms > 500 {
        return "\x1b[33m⚠ slow\x1b[0m".to_string();
    }
    if lower == "ok" || lower == "success" {
        return "\x1b[32m✓ ok\x1b[0m".to_string();
    }
    format!("  {}", lower)
}
