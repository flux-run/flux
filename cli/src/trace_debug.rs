//! `flux trace debug <trace-id>` — step-through production request debugger.
//!
//! Walks the execution graph of a past request span-by-span and shows which
//! database mutations happened at each step.  With `span_id` on every
//! mutation row, Fluxbase can reconstruct the complete backend state at any
//! point *during* execution — not just after the request finishes.
//!
//! ```text
//! $ flux trace debug 9624a58d
//!
//!   endpoint:  POST /auth/signup → create_user
//!   request:   9624a58d  2026-03-11 14:23:07
//!   duration:  1.2s  ·  status: 200 OK  ·  4 steps
//!
//! ─────────────────────────────────────────
//!  Step 1 of 4  ·  db.insert (users)                 42ms
//! ─────────────────────────────────────────
//!   + users.id=42
//!       email:      alice@example.com
//!       plan:       free
//!
//! ─────────────────────────────────────────
//!  Step 2 of 4  ·  stripe.charge                    380ms
//! ─────────────────────────────────────────
//!   no state changes in this step
//!
//! ─────────────────────────────────────────
//!  Step 3 of 4  ·  db.update (users)                 18ms
//! ─────────────────────────────────────────
//!   ~ users.id=42
//!       plan:  free → pro
//!
//! ─────────────────────────────────────────
//!  Step 4 of 4  ·  gmail.send_email                 210ms
//! ─────────────────────────────────────────
//!   no state changes in this step
//!
//! ─────────────────────────────────────────
//!  Final state after request 9624a58d
//! ─────────────────────────────────────────
//!   users.id=42
//!     email: alice@example.com
//!     plan:  pro
//! ```
//!
//! Use `--at <step>` to inspect state at a specific step without printing all steps.
//!
//! ```text
//! $ flux trace debug 9624a58d --at 2
//!
//! State after step 2 (stripe.charge):
//!   users.id=42
//!     email: alice@example.com
//!     plan:  free         ← upgrade hasn't happened yet
//! ```

use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

/// Entry point for `flux trace debug <trace-id> [--at <step>] [--json]`
pub async fn execute(
    trace_id: String,
    at_step:  Option<usize>,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let trace_url = format!("{}/traces/{}?slow_ms=0", client.base_url, trace_id);
    let muts_url  = format!("{}/db/mutations?request_id={}&limit=500", client.base_url, trace_id);

    let (trace, muts) = tokio::try_join!(
        fetch_json(&client, &trace_url),
        fetch_json(&client, &muts_url),
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "trace": trace,
            "mutations": muts,
        }))?);
        return Ok(());
    }

    // ── Extract trace metadata ────────────────────────────────────────────────
    let req = trace.get("request").unwrap_or(&Value::Null);
    let method   = req["method"].as_str().unwrap_or("?");
    let path     = req["path"].as_str().unwrap_or("?");
    let function = req["function"].as_str().unwrap_or("?");
    let status   = req["status"].as_i64().unwrap_or(0);
    let total_ms = trace["total_ms"].as_i64().unwrap_or(0);
    let ts       = req["created_at"].as_str().unwrap_or("?");

    let empty_spans: Vec<Value> = vec![];
    let all_spans = trace["spans"].as_array().unwrap_or(&empty_spans);

    // Filter to "behavioral" spans only — same set diffed by flux trace diff.
    let steps: Vec<&Value> = all_spans.iter()
        .filter(|s| matches!(
            s["span_type"].as_str().unwrap_or(""),
            "tool" | "db" | "workflow_step" | "agent_step"
        ))
        .collect();

    let empty_muts: Vec<Value> = vec![];
    let mut_rows = muts["mutations"].as_array().unwrap_or(&empty_muts);

    // ── Print header ──────────────────────────────────────────────────────────
    println!();
    println!("  {}  {} {} → {}",
        "endpoint:".dimmed(),
        method.bold(), path.bold(),
        function.cyan().bold(),
    );
    let status_str = if status == 0 { "?".to_string() }
                     else if status < 400 { format!("{status} OK").green().to_string() }
                     else { format!("{status} FAILED").red().to_string() };
    println!("  {}  {}  {}",
        "request: ".dimmed(),
        trunc(&trace_id, 12).yellow(),
        ts.dimmed(),
    );
    println!("  {}  {}ms  ·  {}  ·  {} steps",
        "summary: ".dimmed(),
        total_ms,
        status_str,
        steps.len(),
    );
    println!();

    // ── Accumulate state as we walk steps ─────────────────────────────────────
    // state_at[table_name][pk_str] = latest JSONB after_state
    let mut state_at: std::collections::HashMap<String, std::collections::HashMap<String, Value>> =
        std::collections::HashMap::new();

    // Group mutations by span_id (Some) or by position (None — legacy fallback).
    // For legacy rows (no span_id), assign them to whichever step has the
    // matching table + timing — best-effort, falls back to step 1.
    let has_span_ids = mut_rows.iter().any(|m| m["span_id"].is_string());

    let total_steps = steps.len();

    for (i, span) in steps.iter().enumerate() {
        let step_num = i + 1;
        let span_id_val = span["span_id"].as_str().unwrap_or("");
        let span_name = span_label(span);
        let span_ms   = span["duration_ms"].as_i64().unwrap_or(0);

        // Find mutations for this step.
        let step_muts: Vec<&Value> = if has_span_ids {
            mut_rows.iter()
                .filter(|m| m["span_id"].as_str() == Some(span_id_val))
                .collect()
        } else {
            // Legacy fallback: attribute mutations proportionally by position.
            // Distribute evenly across steps.
            let chunk = (mut_rows.len() + total_steps - 1).max(1) / total_steps.max(1);
            let start = i * chunk;
            let end   = ((i + 1) * chunk).min(mut_rows.len());
            mut_rows[start..end].iter().collect()
        };

        // Apply step mutations to the running state.
        for m in &step_muts {
            let table = m["table_name"].as_str().unwrap_or("?").to_string();
            let pk_key = pk_str(&m["record_pk"]);
            let op = m["operation"].as_str().unwrap_or("?");
            let entry = state_at.entry(table).or_default();
            match op {
                "insert" | "update" => {
                    if let Some(after) = m["after_state"].as_object() {
                        entry.insert(pk_key, Value::Object(after.clone()));
                    }
                }
                "delete" => {
                    entry.remove(&pk_key);
                }
                _ => {}
            }
        }

        // If --at is set, skip steps we don't need to print; still apply mutations.
        if let Some(target) = at_step {
            if step_num < target {
                continue; // keep accumulating state, don't print
            }
            if step_num > target {
                break;
            }
        }

        // ── Print step header ─────────────────────────────────────────────────
        println!("{}", "─".repeat(45).dimmed());
        println!(" {}  ·  {}    {}",
            format!("Step {step_num} of {total_steps}").bold(),
            span_name.cyan().bold(),
            if span_ms > 0 { format!("{span_ms}ms").dimmed().to_string() } else { String::new() },
        );
        println!("{}", "─".repeat(45).dimmed());

        if step_muts.is_empty() {
            println!("  {}", "no state changes in this step".dimmed());
        } else {
            for m in &step_muts {
                let table = m["table_name"].as_str().unwrap_or("?");
                let pk_key = pk_str(&m["record_pk"]);
                let op = m["operation"].as_str().unwrap_or("?");

                println!();
                println!("  {} {}.{}",
                    color_op_prefix(op),
                    table.cyan(),
                    pk_key.yellow(),
                );

                match op {
                    "insert" => print_json_fields(&m["after_state"],  4),
                    "update" => print_update_diff(&m["before_state"], &m["after_state"], 4),
                    "delete" => print_json_fields(&m["before_state"], 4),
                    _ => {}
                }
            }
        }

        println!();

        // If --at was given and we just printed the target step, print state snapshot.
        if let Some(target) = at_step {
            if step_num == target {
                println!("{}", "─".repeat(45).dimmed());
                println!(" {}", format!("State after step {step_num} ({span_name})").bold());
                println!("{}", "─".repeat(45).dimmed());
                print_cumulative_state(&state_at);
                return Ok(());
            }
        }
    }

    // ── Final state ───────────────────────────────────────────────────────────
    if at_step.is_none() {
        println!("{}", "─".repeat(45).dimmed());
        println!(" {}", format!("Final state after request {}", trunc(&trace_id, 12)).bold());
        println!("{}", "─".repeat(45).dimmed());
        print_cumulative_state(&state_at);
        println!();
    }

    Ok(())
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn span_label(span: &Value) -> String {
    let kind = span["span_type"].as_str().unwrap_or("span");
    let name = span["data"]["action"].as_str()
        .or_else(|| span["data"]["tool"].as_str())
        .or_else(|| span["data"]["table"].as_str())
        .or_else(|| span["message"].as_str())
        .or_else(|| span["name"].as_str())
        .unwrap_or(kind);
    name.to_string()
}

fn pk_str(pk: &Value) -> String {
    match pk {
        Value::Object(m) => {
            if let Some(id) = m.get("id") {
                return format!("id={}", scalar_str(id));
            }
            // Composite PK: join key=val
            m.iter()
                .map(|(k, v)| format!("{k}={}", scalar_str(v)))
                .collect::<Vec<_>>()
                .join(",")
        }
        other => scalar_str(other),
    }
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b)   => b.to_string(),
        Value::Null      => "null".to_string(),
        other            => other.to_string(),
    }
}

fn color_op_prefix(op: &str) -> colored::ColoredString {
    match op {
        "insert" => "+".green().bold(),
        "update" => "~".yellow().bold(),
        "delete" => "-".red().bold(),
        _        => "·".normal(),
    }
}

fn print_json_fields(v: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            println!("{pad}{}: {}", k.dimmed(), scalar_str(val));
        }
    }
}

fn print_update_diff(before: &Value, after: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    // Show only changed fields
    let before_obj = before.as_object();
    let after_obj  = after.as_object();
    match (before_obj, after_obj) {
        (Some(b), Some(a)) => {
            let mut printed = false;
            for (k, av) in a {
                let bv = b.get(k).unwrap_or(&Value::Null);
                if bv != av {
                    println!("{pad}{}: {} → {}",
                        k.dimmed(),
                        scalar_str(bv).yellow(),
                        scalar_str(av).green(),
                    );
                    printed = true;
                }
            }
            if !printed {
                println!("{pad}{}", "(no field changes detected)".dimmed());
            }
        }
        _ => print_json_fields(after, indent),
    }
}

fn print_cumulative_state(
    state: &std::collections::HashMap<String, std::collections::HashMap<String, Value>>,
) {
    if state.is_empty() {
        println!("  {}", "(no mutations recorded)".dimmed());
        return;
    }
    let mut tables: Vec<&String> = state.keys().collect();
    tables.sort();
    for table in tables {
        let rows = &state[table];
        let mut pks: Vec<&String> = rows.keys().collect();
        pks.sort();
        for pk in pks {
            println!();
            println!("  {}.{}", table.cyan(), pk.yellow());
            print_json_fields(&rows[pk], 4);
        }
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}…", &s[..n]) } else { s.to_string() }
}

async fn fetch_json(client: &ApiClient, url: &str) -> anyhow::Result<Value> {
    let res = client.client.get(url).send().await?;
    if res.status().is_success() {
        Ok(res.json().await.unwrap_or_default())
    } else {
        Ok(Value::Null)
    }
}
