//! `flux debug [request-id]` — the killer debugging command.
//!
//! With no arguments: interactive production debugger.
//!   Lists recent errors → user selects → auto traces + logs + suggests fix.
//!
//! With a request ID: deep-dive a specific request.
//!
//! ```text
//! flux debug                        # interactive: pick from recent errors
//! flux debug 9624a58d57e7           # deep-dive a specific request
//! flux debug 9624a58d57e7 --replay
//! flux debug 9624a58d57e7 --replay-payload override.json
//! flux debug 9624a58d57e7 --no-logs
//! ```

use colored::Colorize;
use serde_json::Value;
use std::io::BufRead;
use std::io::{self, Write};

use crate::client::ApiClient;

/// Entry point — routes to interactive mode or direct mode.
pub async fn execute(
    request_id: Option<String>,
    replay: bool,
    replay_payload: Option<String>,
    no_logs: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    match request_id {
        None => execute_interactive().await,
        Some(id) => execute_request(id, replay, replay_payload, no_logs, json_output, false).await,
    }
}

/// Interactive mode: show recent errors, let user pick one.
async fn execute_interactive() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Fetch recent errors (traces with errors, last 10 minutes)
    let res = client
        .client
        .get(format!(
            "{}/traces?status=error&limit=15&window=10m",
            client.base_url
        ))
        .send()
        .await?;

    let body: Value = res.json().await.unwrap_or_default();
    let empty = vec![];
    let errors: &Vec<Value> = body
        .get("data")
        .and_then(|d| d.as_array())
        .unwrap_or(&empty);

    if errors.is_empty() {
        println!(
            "{}",
            "No production errors in the last 10 minutes.".green().bold()
        );
        println!("{}", "Your backend looks healthy! ✔".dimmed());
        return Ok(());
    }

    println!();
    println!("{}", "Recent Production Errors (last 10m)".bold());
    println!("{}", "─".repeat(52).dimmed());

    for (i, e) in errors.iter().enumerate() {
        let route = e["route"].as_str().unwrap_or("?");
        let function = e["function"].as_str().unwrap_or("?");
        let error = e["error"].as_str().unwrap_or("unknown");
        let request_id = e["request_id"].as_str().unwrap_or("?");
        let duration = e["total_ms"].as_i64().unwrap_or(0);

        let dur_str = if duration > 1000 {
            format!("{}s", duration / 1000).yellow().to_string()
        } else {
            format!("{}ms", duration).normal().to_string()
        };

        println!(
            "{}) {}  {}  {}",
            (i + 1).to_string().bold().cyan(),
            route.bold(),
            function.dimmed(),
            error.red()
        );
        println!(
            "   request_id: {}  {}",
            request_id.cyan(),
            dur_str
        );
        println!();
    }

    // Prompt
    print!("{}", "Select an error to inspect › ".bold());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let input = input.trim();

    let idx: usize = match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= errors.len() => n - 1,
        _ => {
            eprintln!("{} Invalid selection.", "✗".red().bold());
            return Ok(());
        }
    };

    let selected_id = errors[idx]["request_id"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if selected_id.is_empty() {
        eprintln!("{} Could not read request_id for that entry.", "✗".red().bold());
        return Ok(());
    }

    println!();
    execute_request(selected_id, false, None, false, false, false).await
}

/// Called by `flux tail --auto-debug` — full debug output without the replay prompt.
pub async fn execute_auto(request_id: String) -> anyhow::Result<()> {
    execute_request(request_id, false, None, false, false, true).await
}

async fn execute_request(
    request_id: String,
    replay: bool,
    replay_payload: Option<String>,
    no_logs: bool,
    json_output: bool,
    skip_prompt: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // ── 1. Fetch trace ────────────────────────────────────────────────────
    let trace_res = client
        .client
        .get(format!("{}/traces/{}", client.base_url, request_id))
        .send()
        .await?;

    let trace_json: Value = trace_res.json().await.unwrap_or_default();
    let trace = trace_json.get("data").unwrap_or(&trace_json);

    // ── 2. Fetch logs ─────────────────────────────────────────────────────
    let logs_data: Vec<Value> = if !no_logs {
        let logs_res = client
            .client
            .get(format!(
                "{}/logs?request_id={}&limit=100",
                client.base_url, request_id
            ))
            .send()
            .await
            .ok();

        if let Some(res) = logs_res {
            let lj: Value = res.json().await.unwrap_or_default();
            lj.get("data")
                .and_then(|d| d.get("logs"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "trace": trace,
                "logs": logs_data,
            }))?
        );
        return Ok(());
    }

    // ── 3. Print Request Summary ──────────────────────────────────────────
    println!();
    println!("{}", "Request Summary".bold());
    println!("{}", "─".repeat(45).dimmed());

    let summary_w = 14usize;
    let route = trace["route"].as_str().unwrap_or("?");
    let function = trace["function"].as_str().unwrap_or("?");
    let total_ms = trace["total_ms"].as_i64().unwrap_or(0);
    let status_val = trace["status"].as_str().unwrap_or("?");
    let time_val = trace["created_at"]
        .as_str()
        .map(fmt_ts)
        .unwrap_or("?".to_string());

    println!(
        "{:<summary_w$} {}",
        "Request ID:".bold(),
        request_id.cyan()
    );
    println!("{:<summary_w$} {}", "Route:".bold(), route);
    println!("{:<summary_w$} {}", "Function:".bold(), function);
    println!("{:<summary_w$} {}ms", "Duration:".bold(), total_ms);
    println!(
        "{:<summary_w$} {}",
        "Status:".bold(),
        colorize_status(status_val)
    );
    println!("{:<summary_w$} {}", "Time:".bold(), time_val.dimmed());

    // Trace URL — shareable link for Slack / issue comments
    let trace_url = {
        let slug = client.config.tenant_slug.as_deref().unwrap_or("");
        let proj = client.config.project_id.as_deref().unwrap_or("");
        if !slug.is_empty() && !proj.is_empty() {
            format!("https://app.fluxbase.co/{}/{}/traces/{}", slug, proj, request_id)
        } else {
            format!("https://app.fluxbase.co/traces/{}", request_id)
        }
    };
    println!("{:<summary_w$} {}", "Trace URL:".bold(), trace_url.cyan().underline());

    // ── 4. Print Trace spans ──────────────────────────────────────────────
    println!();
    println!("{}", "Trace".bold());
    println!("{}", "─".repeat(45).dimmed());

    let spans = trace["spans"].as_array();
    if let Some(spans) = spans {
        let name_w = spans
            .iter()
            .map(|s| {
                let src = s["source"].as_str().unwrap_or("");
                let res = s["resource"].as_str().unwrap_or("");
                if res.is_empty() {
                    src.len()
                } else {
                    src.len() + 1 + res.len()
                }
            })
            .max()
            .unwrap_or(20)
            .clamp(10, 32);

        for span in spans {
            let source = span["source"].as_str().unwrap_or("?");
            let resource = span["resource"].as_str().unwrap_or("");
            let span_name = if resource.is_empty() {
                source.to_string()
            } else {
                format!("{}.{}", source, resource)
            };
            let duration = span["duration_ms"]
                .as_i64()
                .or_else(|| span["delta_ms"].as_i64())
                .unwrap_or(0);
            let span_type = span["span_type"].as_str().unwrap_or("event");
            let is_error = span_type == "error"
                || span["error"].as_str().map(|e| !e.is_empty()).unwrap_or(false);

            let marker = if is_error {
                "✗".red().bold().to_string()
            } else {
                "✔".green().bold().to_string()
            };

            let error_note = if is_error {
                let err = span["error"].as_str().unwrap_or("error");
                format!("  {}", err.red())
            } else {
                String::new()
            };

            println!(
                "{:<name_w$}  {}ms  {}{}",
                span_name,
                duration,
                marker,
                error_note
            );
        }
    } else {
        println!("  {}", "(no spans recorded)".dimmed());
    }

    // ── 5. Print Logs ─────────────────────────────────────────────────────
    if !no_logs {
        println!();
        println!("{}", "Logs".bold());
        println!("{}", "─".repeat(45).dimmed());

        if logs_data.is_empty() {
            println!("  {}", "(no logs for this request)".dimmed());
        } else {
            for entry in &logs_data {
                let ts = entry["timestamp"].as_str().map(|t| t.get(..19).unwrap_or(t)).unwrap_or("?");
                let source = entry["source"].as_str().unwrap_or("?");
                let level = entry["level"].as_str().unwrap_or("info");
                let msg = entry["message"].as_str().unwrap_or("");

                let level_col = match level.to_uppercase().as_str() {
                    "ERROR" | "ERR" => level.to_uppercase().red().bold().to_string(),
                    "WARN" => level.to_uppercase().yellow().bold().to_string(),
                    "DEBUG" => level.to_uppercase().dimmed().to_string(),
                    _ => level.to_uppercase().normal().to_string(),
                };

                println!("[{}]  {}  {}  {}", ts.dimmed(), source.green(), level_col, msg);
            }
        }
    }

    // ── 6. Suggested Fix (based on error spans) ───────────────────────────
    if let Some(spans) = trace["spans"].as_array() {
        let error_spans: Vec<&Value> = spans
            .iter()
            .filter(|s| {
                s["span_type"].as_str() == Some("error")
                    || s["error"].as_str().map(|e| !e.is_empty()).unwrap_or(false)
            })
            .collect();

        if !error_spans.is_empty() {
            println!();
            println!("{}", "Suggested Fix".bold());
            println!("{}", "─".repeat(45).dimmed());

            for span in &error_spans {
                let source = span["source"].as_str().unwrap_or("?");
                let resource = span["resource"].as_str().unwrap_or("");
                let error = span["error"].as_str().unwrap_or("unknown error");

                println!(
                    "{} {}.{} error: {}",
                    "⚠".yellow().bold(),
                    source,
                    resource,
                    error.yellow()
                );
                print_suggestion(source, resource, error);
            }
        }
    }

    // ── 7. Replay prompt ─────────────────────────────────────────────────
    if replay {
        do_replay(&client, &request_id, replay_payload.as_deref()).await?;
    } else if !skip_prompt {
        println!();
        print!("{}", "Replay this request? [y/N]: ".bold());
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).ok();
        if line.trim().to_lowercase() == "y" {
            do_replay(&client, &request_id, None).await?;
        }
    }

    Ok(())
}

fn print_suggestion(source: &str, resource: &str, error: &str) {
    let combined = format!("{}.{}", source, resource);
    let error_lower = error.to_lowercase();

    if combined.contains("gmail") || combined.contains("email") {
        if error_lower.contains("rate_limit") || error_lower.contains("rate limit") {
            println!(
                "  {} Queue the email job instead of calling inline.",
                "→".cyan()
            );
            println!("  {} {}", "→".cyan(), "flux queue create email-jobs".bold());
            println!(
                "  {} {}",
                "→".cyan(),
                "flux queue bind email-jobs --function send_email".bold()
            );
        } else if error_lower.contains("auth") || error_lower.contains("token") {
            println!(
                "  {} Re-connect the Gmail integration:",
                "→".cyan()
            );
            println!("  {} {}", "→".cyan(), "flux tool connect gmail".bold());
        }
    } else if source == "db" {
        if error_lower.contains("unique") || error_lower.contains("duplicate") {
            println!("  {} Handle duplicate key error in your function.", "→".cyan());
        } else if error_lower.contains("timeout") {
            println!("  {} Add an index or optimise the query.", "→".cyan());
        }
    } else if error_lower.contains("timeout") {
        println!(
            "  {} Increase or add a timeout in your function.",
            "→".cyan()
        );
        println!(
            "  {} Check recent logs: {}",
            "→".cyan(),
            format!("flux logs --request-id {}", "…").bold()
        );
    }
}

async fn do_replay(
    client: &ApiClient,
    request_id: &str,
    payload_path: Option<&str>,
) -> anyhow::Result<()> {
    let body = if let Some(path) = payload_path {
        let raw = tokio::fs::read_to_string(path).await.map_err(|e| {
            anyhow::anyhow!("Could not read payload file '{}': {}", path, e)
        })?;
        let json: Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("Payload file is not valid JSON: {}", e))?;
        println!("  using custom payload: {}", path.dimmed());
        json
    } else {
        serde_json::json!({})
    };

    let res = client
        .client
        .post(format!("{}/traces/{}/replay", client.base_url, request_id))
        .json(&body)
        .send()
        .await?;

    if res.status().is_success() {
        let rj: Value = res.json().await.unwrap_or_default();
        let data = rj.get("data").unwrap_or(&rj);
        let new_id = data["request_id"].as_str().unwrap_or("?");
        println!("  new request_id: {}", new_id.cyan());
    } else {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        eprintln!("{} Replay failed: {} — {}", "✗".red().bold(), status, body);
    }

    Ok(())
}

fn fmt_ts(ts: &str) -> String {
    ts.get(..19)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| ts.to_string())
        + " UTC"
}

fn colorize_status(status: &str) -> colored::ColoredString {
    match status {
        "success" | "ok" => status.green().bold(),
        "error" | "failed" => status.red().bold(),
        "pending" => status.yellow().bold(),
        _ => status.normal(),
    }
}
