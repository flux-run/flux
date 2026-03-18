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
        trace.method, trace.path, trace.status, trace.duration_ms
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
        let annotation = checkpoint_annotation(&cp.boundary, &res);

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
                cp.call_index, requested_delay_ms, effective_delay_ms,
            );

            if args.verbose {
                let request_json =
                    serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json =
                    serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

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
                let request_json =
                    serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json =
                    serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

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
            let tls = req
                .get("tls")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let row_count = res
                .get("row_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);

            println!(
                "  [{}] POSTGRES{}  {}:{}  {}ms  → {} rows  {}",
                cp.call_index,
                if tls { "+TLS" } else { "" },
                host,
                port,
                cp.duration_ms,
                row_count,
                sql,
            );

            if args.verbose {
                let request_json =
                    serde_json::to_string(&req).unwrap_or_else(|_| "null".to_string());
                let response_json =
                    serde_json::to_string(&res).unwrap_or_else(|_| "null".to_string());

                println!("      request");
                print_json_block(&request_json, true);
                println!("      response");
                print_json_block(&response_json, true);
            }
            continue;
        }

        println!(
            "  [{}] {}  {}  {}ms  → {}{}",
            cp.call_index,
            cp.boundary.to_uppercase(),
            url,
            cp.duration_ms,
            status,
            annotation
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

fn checkpoint_annotation(boundary: &str, response: &serde_json::Value) -> String {
    if boundary != "http" {
        return String::new();
    }

    let Some(cache) = response.get("cache") else {
        return String::new();
    };

    let hit = cache
        .get("hit")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !hit {
        return String::new();
    }

    let source = cache
        .get("source")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");

    let age = cache
        .get("age_ms")
        .and_then(|value| value.as_u64())
        .map(format_cache_age);

    if let Some(age) = age {
        return format!("  cache hit ({source}, age={age})");
    }

    format!("  cache hit ({source})")
}

fn format_cache_age(age_ms: u64) -> String {
    if age_ms < 1_000 {
        return "<1s".to_string();
    }

    let total_seconds = age_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        if minutes > 0 {
            return format!("{hours}h {minutes}m");
        }
        return format!("{hours}h");
    }

    if minutes > 0 {
        if seconds > 0 {
            return format!("{minutes}m {seconds}s");
        }
        return format!("{minutes}m");
    }

    format!("{seconds}s")
}

fn print_json_block(raw: &str, expanded: bool) {
    if !expanded {
        println!("    (hidden, use --verbose)");
        return;
    }

    let value = serde_json::from_str::<serde_json::Value>(raw)
        .unwrap_or(serde_json::Value::String(raw.to_string()));
    let formatted = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string());
    for line in formatted.lines() {
        println!("    {}", line);
    }
}

#[cfg(test)]
mod tests {
    use super::{checkpoint_annotation, format_cache_age};

    #[test]
    fn shows_memory_cache_hit_for_http_checkpoint() {
        let response = serde_json::json!({
            "status": 200,
            "cache": {
                "hit": true,
                "source": "memory",
                "age_ms": 123_456
            }
        });

        assert_eq!(
            checkpoint_annotation("http", &response),
            "  cache hit (memory, age=2m 3s)"
        );
    }

    #[test]
    fn falls_back_when_cache_age_is_missing() {
        let response = serde_json::json!({
            "cache": {
                "hit": true,
                "source": "memory"
            }
        });

        assert_eq!(
            checkpoint_annotation("http", &response),
            "  cache hit (memory)"
        );
    }

    #[test]
    fn hides_annotation_for_non_http_checkpoint() {
        let response = serde_json::json!({
            "cache": {
                "hit": true,
                "source": "memory"
            }
        });

        assert_eq!(checkpoint_annotation("postgres", &response), "");
    }

    #[test]
    fn hides_annotation_for_cache_miss() {
        let response = serde_json::json!({
            "cache": {
                "hit": false,
                "source": "memory"
            }
        });

        assert_eq!(checkpoint_annotation("http", &response), "");
    }

    #[test]
    fn formats_subsecond_cache_age() {
        assert_eq!(format_cache_age(999), "<1s");
    }

    #[test]
    fn formats_hour_scale_cache_age() {
        assert_eq!(format_cache_age(3_780_000), "1h 3m");
    }
}
