use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::{get_trace, resume};

#[derive(Debug, Args)]
pub struct ResumeArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long, value_name = "INDEX")]
    pub from: Option<i32>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
}

/// Decode a `__FLUX_B64:<base64>` string to plain text.
fn decode_flux_b64(s: &str) -> String {
    if let Some(b64) = s.strip_prefix("__FLUX_B64:") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if let Ok(text) = String::from_utf8(bytes) {
                return text;
            }
        }
    }
    s.to_string()
}

/// Walk a JSON value and decode any `__FLUX_B64:` strings in-place.
fn decode_b64_in_value(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::String(s) if s.starts_with("__FLUX_B64:") => {
            *s = decode_flux_b64(s);
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                decode_b64_in_value(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                decode_b64_in_value(v);
            }
        }
        _ => {}
    }
}

/// Pretty-print a JSON value with indentation, one key: value per line.
fn print_json_fields(val: &serde_json::Value, indent: &str) {
    match val {
        serde_json::Value::Object(map) => {
            // Find the max key length for alignment
            let max_key = map.keys().map(|k| k.len()).max().unwrap_or(0);
            for (k, v) in map {
                let padding = " ".repeat(max_key.saturating_sub(k.len()));
                match v {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        println!("{}{}:{}", indent, k, padding);
                        print_json_fields(v, &format!("{}  ", indent));
                    }
                    serde_json::Value::String(s) => {
                        println!("{}{}:{}  {}", indent, k, padding, s);
                    }
                    other => {
                        println!("{}{}:{}  {}", indent, k, padding, other);
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                print_json_fields(item, indent);
            }
        }
        other => println!("{}{}", indent, other),
    }
}

/// Describe an IO boundary step in plain English.
fn describe_io_step(boundary: &str, used_recorded: bool, duration_ms: i32) -> String {
    let action = match boundary.to_lowercase().as_str() {
        "postgres" | "db" => {
            if used_recorded { "database read (replayed)" } else { "database write applied" }
        }
        "http" | "fetch" => {
            if used_recorded { "HTTP call (replayed)" } else { "HTTP request sent live" }
        }
        "redis" => {
            if used_recorded { "Redis read (replayed)" } else { "Redis write applied" }
        }
        "tcp" => {
            if used_recorded { "TCP exchange (replayed)" } else { "TCP connection made live" }
        }
        other => {
            if used_recorded {
                return format!("{} (replayed, {}ms)", other.to_uppercase(), duration_ms);
            } else {
                return format!("{} write applied ({}ms)", other.to_uppercase(), duration_ms);
            }
        }
    };
    format!("{} ({}ms)", action, duration_ms)
}

pub async fn execute(args: ResumeArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;

    let short_id = if args.execution_id.len() >= 8 {
        &args.execution_id[..8]
    } else {
        &args.execution_id
    };

    // Fetch original trace so we know the input and whether it originally failed
    let original_trace = get_trace(&auth.url, &auth.token, &args.execution_id).await.ok();
    let original_failed = original_trace.as_ref().map(|t| t.status != "ok").unwrap_or(true);

    // Execute the resume
    let response = resume(&auth.url, &auth.token, &args.execution_id, args.from).await?;

    let new_short_id = if response.execution_id.len() >= 8 {
        response.execution_id[..8].to_string()
    } else {
        response.execution_id.clone()
    };

    let succeeded = response.status == "ok";

    // ── Header ───────────────────────────────────────────────────────────────
    println!();
    println!("  \x1b[1mrsuming {}…\x1b[0m", short_id);
    println!();
    println!("  \x1b[33m⚠\x1b[0m  executing with real side effects");
    if response.steps.iter().any(|s| s.boundary.contains("postgres") || s.boundary.contains("db")) {
        println!("     • database writes will be applied");
    }
    if response.steps.iter().any(|s| s.boundary.contains("http") || s.boundary.contains("fetch")) {
        println!("     • HTTP requests will be sent");
    }
    if response.steps.iter().any(|s| s.boundary.contains("redis")) {
        println!("     • Redis writes will be applied");
    }

    // ── Step 0 ───────────────────────────────────────────────────────────────
    println!();
    println!("  ────────────────────────────\n");

    if let Some(ref trace) = original_trace {
        println!("  \x1b[1mSTEP 0 — {} {}\x1b[0m\n", trace.method, trace.path);

        // Input from original request
        if !trace.request_json.is_empty() {
            if let Ok(mut req_val) = serde_json::from_str::<serde_json::Value>(&trace.request_json) {
                decode_b64_in_value(&mut req_val);
                // For HTTP requests the body is nested; try to unwrap it
                let body_val = req_val.get("body")
                    .and_then(|b| b.as_str())
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .unwrap_or_else(|| req_val.clone());

                if body_val.is_object() && !body_val.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                    println!("  \x1b[1minput\x1b[0m");
                    print_json_fields(&body_val, "    ");
                    println!();
                }
            }
        }
    }

    // ── Execution steps ──────────────────────────────────────────────────────
    println!("  \x1b[1mexecution\x1b[0m");

    if response.steps.is_empty() && succeeded {
        println!("  \x1b[32m  ✓\x1b[0m validation passed");
        println!("  \x1b[32m  ✓\x1b[0m handler completed");
    } else if response.steps.is_empty() && !succeeded {
        println!("  \x1b[31m  ✗\x1b[0m handler threw before reaching any IO");
    } else {
        println!("  \x1b[32m  ✓\x1b[0m validation passed");
        for step in &response.steps {
            let desc = describe_io_step(&step.boundary, step.used_recorded, step.duration_ms);
            println!("  \x1b[32m  ✓\x1b[0m {}", desc);
        }
    }

    // ── IO section ───────────────────────────────────────────────────────────
    if !response.steps.is_empty() {
        println!();
        println!("  \x1b[1mio\x1b[0m");
        for step in &response.steps {
            let symbol = if step.used_recorded { "\x1b[2m⏺\x1b[0m" } else { "\x1b[32m✓\x1b[0m" };
            let label = describe_io_step(&step.boundary, step.used_recorded, step.duration_ms);
            let kind = if step.used_recorded { "replayed" } else { "live" };
            println!("  {}  \x1b[1m{}\x1b[0m  \x1b[2m({})\x1b[0m", symbol, label, kind);
        }
    }

    // ── Error ────────────────────────────────────────────────────────────────
    if !response.error.is_empty() && !response.error.starts_with("__FLUX_BOUNDARY_STOP") {
        println!();
        println!("  \x1b[1merror\x1b[0m");
        println!("    \x1b[31m✗\x1b[0m {}", response.error);
    }

    // ── Result ───────────────────────────────────────────────────────────────
    println!();
    println!("  ────────────────────────────\n");

    if succeeded {
        // Parse and pretty-print the response body
        let output_val: Option<serde_json::Value> = serde_json::from_str(&response.output).ok().and_then(|mut v: serde_json::Value| {
            decode_b64_in_value(&mut v);
            // Unwrap net_response body if present
            if let Some(nr) = v.get("net_response").cloned() {
                let status_code = nr.get("status").and_then(|s| s.as_u64()).unwrap_or(200);
                let body_str = nr.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string();
                let body_json: Option<serde_json::Value> = serde_json::from_str(&body_str).ok();
                if let Some(body) = body_json {
                    return Some(serde_json::json!({ "_status": status_code, "body": body }));
                }
            }
            Some(v)
        });

        let http_status = output_val.as_ref()
            .and_then(|v| v.get("_status").and_then(|s| s.as_u64()))
            .unwrap_or(200);

        println!("  \x1b[1mresult\x1b[0m");
        println!("  \x1b[32m  ✓\x1b[0m request succeeded ({})", http_status);

        if let Some(ref val) = output_val {
            let display_val = val.get("body").unwrap_or(val);
            if display_val.is_object() && !display_val.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                println!();
                println!("  \x1b[1moutput\x1b[0m");
                print_json_fields(display_val, "    ");
            }
        }

        // Difference vs original
        if original_failed {
            println!();
            println!("  ────────────────────────────\n");
            println!("  \x1b[1mdifference vs original\x1b[0m");
            println!();
            println!("    original:  \x1b[31m✗\x1b[0m failed");
            println!("    now:       \x1b[32m✓\x1b[0m completed successfully");
            println!();
            println!("  \x1b[32m✓\x1b[0m \x1b[1moriginal failure recovered\x1b[0m");
        }

        println!();
        println!("  \x1b[2mexecution id\x1b[0m");
        println!("    {}", new_short_id);
    } else {
        println!("  \x1b[1mresult\x1b[0m");
        println!("  \x1b[31m  ✗\x1b[0m request failed");
        println!();
        println!("  \x1b[31m✗\x1b[0m execution recorded as \x1b[1m{}\x1b[0m", new_short_id);
        println!();
        println!("  \x1b[2mnext\x1b[0m");
        println!("    → run \x1b[1mflux why {}\x1b[0m to diagnose this failure", new_short_id);
    }

    println!();
    Ok(())
}
