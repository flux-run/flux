//! `flux why <request-id>` — root cause explanation for a failed or anomalous request.
//!
//! Combines trace spans, error logs, and state mutations from a single request_id
//! into one human-readable causal summary:
//!
//! ```text
//! flux why 550e8400
//!
//! ❌  POST /signup  →  create_user  (142ms, FAILED)
//!     request_id:  550e8400-e29b-41d4-a716-446655440000
//!     commit:      a93f42c
//!     error:       TypeError: Cannot read properties of undefined (reading 'id')
//!                  at create_user/index.ts:42
//!
//! ─── State changes ─────────────────────────────────────────────────────────
//!   1 mutation  (users INSERT)
//!   users  v1   INSERT  id=7f3a…  by api-key  [request 550e8400]
//!
//! ─── Suggested next steps ──────────────────────────────────────────────────
//!   flux debug 550e8400           deep-dive the full trace
//!   flux state history users 7f3a  full row version history
//! ```

use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n {
        format!("{}…", &s[..n])
    } else {
        s.to_string()
    }
}

pub async fn execute(request_id: String, json_output: bool) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // ── Fetch trace spans ────────────────────────────────────────────────────
    let trace_url = format!("{}/traces/{}?slow_ms=0", client.base_url, request_id);
    let trace_res = client.client.get(&trace_url).send().await?;
    let trace_body: Value = if trace_res.status().is_success() {
        trace_res.json().await.unwrap_or_default()
    } else {
        Value::Null
    };

    // ── Fetch state mutations ────────────────────────────────────────────────
    let mut_url = format!(
        "{}/db/mutations?request_id={}&limit=20",
        client.base_url, request_id
    );
    let mut_res = client.client.get(&mut_url).send().await?;
    let mut_body: Value = if mut_res.status().is_success() {
        mut_res.json().await.unwrap_or_default()
    } else {
        Value::Null
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "trace": trace_body,
                "mutations": mut_body,
            }))?
        );
        return Ok(());
    }

    // ── Extract trace metadata ───────────────────────────────────────────────
    let empty_vec = vec![];
    let spans: &Vec<Value> = trace_body
        .get("spans")
        .and_then(|s| s.as_array())
        .unwrap_or(&empty_vec);

    let request   = trace_body.get("request").unwrap_or(&Value::Null);
    let method    = request["method"].as_str().unwrap_or("?");
    let path      = request["path"].as_str().unwrap_or("?");
    let function  = request["function"].as_str().unwrap_or("?");
    let commit    = request["code_sha"].as_str().unwrap_or("?");
    let status    = request["status"].as_i64().unwrap_or(0);
    let elapsed   = trace_body["total_ms"].as_i64().unwrap_or(0);
    let is_error  = status >= 400 || spans.iter().any(|s| s["span_type"] == "error");

    // ── Find error spans ─────────────────────────────────────────────────────
    let error_spans: Vec<&Value> = spans.iter()
        .filter(|s| s["span_type"] == "error")
        .collect();

    let first_error = error_spans.first();

    println!();

    // ── Header ───────────────────────────────────────────────────────────────
    let status_icon = if is_error { "✗".red().bold() } else { "✔".green().bold() };
    let status_label = if is_error {
        format!("{} FAILED", status).red().bold().to_string()
    } else {
        format!("{} OK", status).green().to_string()
    };

    println!(
        "{}  {} {} → {}  ({}ms, {})",
        status_icon,
        method.bold(),
        path.bold(),
        function.cyan().bold(),
        elapsed,
        status_label,
    );
    println!(
        "    {}  {}",
        "request_id:".dimmed(),
        request_id.dimmed(),
    );

    if commit != "?" && !commit.is_empty() {
        println!(
            "    {}  {}",
            "commit:    ".dimmed(),
            &commit[..commit.len().min(7)],
        );
    }

    // ── Error detail ─────────────────────────────────────────────────────────
    if let Some(err_span) = first_error {
        let msg  = err_span["message"].as_str().unwrap_or("");
        let src  = err_span["resource"].as_str().unwrap_or("");
        println!();
        println!("    {}  {}", "error:     ".dimmed(), msg.red());
        if !src.is_empty() && src != msg {
            println!("    {}  {}", "           ".dimmed(), src.dimmed());
        }
    }

    // ── State mutations ──────────────────────────────────────────────────────
    let mutations = mut_body
        .get("mutations")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    println!();
    if mutations.is_empty() {
        println!(
            "{}",
            "─── State changes ──────────────────────────────────────────────────────────".dimmed()
        );
        println!("  {}", "no state mutations recorded for this request".dimmed());
    } else {
        println!(
            "{} {} {}",
            "─── State changes".bold(),
            format!("({} mutation{})", mutations.len(), if mutations.len() == 1 { "" } else { "s" }).dimmed(),
            "─────────────────────────────────────────────────────".dimmed(),
        );
        for m in &mutations {
            let table   = m["table_name"].as_str().unwrap_or("?");
            let op      = m["operation"].as_str().unwrap_or("?");
            let version = m["version"].as_i64().unwrap_or(0);
            let actor   = m["actor_id"].as_str().unwrap_or("?");

            // Grab a concise row ID from after_state or before_state.
            let row_id = m["after_state"]
                .get("id")
                .or_else(|| m["before_state"].get("id"))
                .and_then(|v| {
                    if let Some(s) = v.as_str() {
                        Some(trunc(s, 8))
                    } else if let Some(n) = v.as_i64() {
                        Some(n.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            let op_colored = match op {
                "insert" => op.green().bold(),
                "update" => op.yellow().bold(),
                "delete" => op.red().bold(),
                _        => op.normal(),
            };

            let row_suffix = if row_id.is_empty() {
                String::new()
            } else {
                format!("  id={}", row_id.dimmed())
            };

            println!(
                "  {}  v{}  {}  by {}{}",
                table.cyan(),
                version,
                op_colored,
                actor.dimmed(),
                row_suffix,
            );
        }
    }

    // ── Suggested next steps ─────────────────────────────────────────────────
    println!();
    println!(
        "{}",
        "─── Suggested next steps ───────────────────────────────────────────────────".dimmed()
    );
    println!(
        "  {}  {}",
        format!("flux debug {}", &request_id[..request_id.len().min(12)]).cyan(),
        "deep-dive the full trace + logs".dimmed(),
    );

    // If there were mutations, suggest state history for the first mutated table
    if let Some(first_mut) = mutations.first() {
        let table = first_mut["table_name"].as_str().unwrap_or("?");
        let row_hint = first_mut["after_state"]
            .get("id")
            .and_then(|v| v.as_str().or_else(|| None))
            .map(|s| format!(" {}", trunc(s, 8)))
            .or_else(|| {
                first_mut["after_state"]
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .map(|n| format!(" {}", n))
            })
            .unwrap_or_default();
        println!(
            "  {}  {}",
            format!("flux state history {} {}", table, row_hint.trim()).cyan(),
            "full row version history".dimmed(),
        );
    }

    println!();
    Ok(())
}
