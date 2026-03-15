//! `flux trace debug <trace-id>` — production-grade step-through request debugger.
//!
//! Improvements: deterministic span ordering, O(1) mutation lookup, structured Step type,
//! interactive mode, state snapshot cache, span status coloring, timing bars, parallel
//! span detection, hidden-mutation warnings, safer JSON access, terminal-width adaptation.
//!
//! ```text
//! $ flux trace debug 9624a58d
//! $ flux trace debug 9624a58d --at 2
//! $ flux trace debug 9624a58d --interactive
//! $ flux trace debug 9624a58d --json
//! ```

use colored::Colorize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead};

use api_contract::routes as R;
use crate::client::ApiClient;

// ── Structured Step (item 17) ─────────────────────────────────────────────────

#[derive(Debug)]
struct Step {
    span_id:     String,
    name:        String,
    duration_ms: i64,
    status:      String,
    start_time:  String,
    span_type:   String,
    tool:        Option<String>,
    table:       Option<String>,
    query:       Option<String>,
    parent_id:   Option<String>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(
    trace_id:    String,
    at_step:     Option<usize>,
    interactive: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let trace_url = format!("{}?slow_ms=0", R::logs::TRACE_GET.url_with(&client.base_url, &[("request_id", trace_id.as_str())]));
    let muts_url  = format!("{}?request_id={}&limit=500", R::db::MUTATIONS.url(&client.base_url), trace_id);

    let (trace, muts) = tokio::try_join!(
        fetch_json(&client, &trace_url),
        fetch_json(&client, &muts_url),
    )?;

    // ── Build sorted step list (item 1) ───────────────────────────────────────
    let empty_spans: Vec<Value> = vec![];
    let raw_spans = trace["spans"].as_array().unwrap_or(&empty_spans);

    let mut sorted_spans: Vec<&Value> = raw_spans.iter().collect();
    sorted_spans.sort_by_key(|s| gstr(s, "start_time").to_string());

    // Item 10: guard against runaway traces (>500 steps)
    let capped = sorted_spans.len() > 500;
    if capped { sorted_spans.truncate(500); }

    let steps: Vec<Step> = sorted_spans.iter()
        .filter(|s| matches!(
            gstr(s, "span_type"),
            "tool" | "db" | "workflow_step" | "agent_step"
        ))
        .map(|s| Step {
            span_id:     gstr(s, "span_id").to_string(),
            name:        span_label(s),
            duration_ms: s["duration_ms"].as_i64().unwrap_or(0),
            status:      gstr(s, "status").to_string(),
            start_time:  gstr(s, "start_time").to_string(),
            span_type:   gstr(s, "span_type").to_string(),
            tool:        s["data"]["tool"].as_str().map(str::to_string),
            table:       s["data"]["table"].as_str().map(str::to_string),
            query:       s["data"]["query"].as_str()
                             .or_else(|| s["data"]["sql"].as_str())
                             .map(str::to_string),
            parent_id:   s["parent_span_id"].as_str().map(str::to_string),
        })
        .collect();

    // ── Build mutation index (item 11) ────────────────────────────────────────
    let empty_muts: Vec<Value> = vec![];
    let mut_rows = muts["mutations"].as_array().unwrap_or(&empty_muts);
    let has_span_ids = mut_rows.iter().any(|m| m["span_id"].is_string());

    let mut mut_by_span: HashMap<String, Vec<&Value>> = HashMap::new();
    let total_steps = steps.len();
    // Item 2: round-robin legacy fallback when span_id column absent
    let mut legacy_map: Vec<Vec<&Value>> = vec![vec![]; total_steps.max(1)];

    if has_span_ids {
        for m in mut_rows {
            if let Some(sid) = m["span_id"].as_str() {
                mut_by_span.entry(sid.to_string()).or_default().push(m);
            }
        }
    } else {
        let n = total_steps.max(1);
        for (idx, m) in mut_rows.iter().enumerate() {
            legacy_map[idx % n].push(m);
        }
    }

    // Item 12: max duration for proportional timing bars
    let max_ms = steps.iter().map(|s| s.duration_ms).max().unwrap_or(1).max(1);

    // ── JSON output (item 13) ─────────────────────────────────────────────────
    if json_output {
        let mut state_json: HashMap<String, HashMap<String, Value>> = HashMap::new();
        let steps_json: Vec<serde_json::Value> = steps.iter().enumerate().map(|(i, step)| {
            let step_muts = get_step_muts(i, step, has_span_ids, &mut_by_span, &legacy_map);
            apply_mutations(&step_muts, &mut state_json);
            serde_json::json!({
                "step":        i + 1,
                "span_id":     step.span_id,
                "name":        step.name,
                "duration_ms": step.duration_ms,
                "status":      step.status,
                "mutations":   step_muts,
            })
        }).collect();
        let final_state: Vec<serde_json::Value> = state_json.iter().flat_map(|(table, rows)| {
            rows.iter().map(move |(pk, v)| serde_json::json!({
                "table": table, "pk": pk, "state": v,
            }))
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "trace_id":    trace_id,
            "request":     trace.get("request").unwrap_or(&Value::Null),
            "total_ms":    trace["total_ms"],
            "steps":       steps_json,
            "final_state": final_state,
        }))?);
        return Ok(());
    }

    // ── Terminal header ───────────────────────────────────────────────────────
    let req      = trace.get("request").unwrap_or(&Value::Null);
    let method   = gstr(req, "method");
    let path     = gstr(req, "path");
    let function = gstr(req, "function");
    let status   = req["status"].as_i64().unwrap_or(0);
    let total_ms = trace["total_ms"].as_i64().unwrap_or(0);
    let ts       = gstr(req, "created_at");
    let tw       = term_width();  // item 18

    println!();
    println!("  {}  {} {} \u{2192} {}",
        "endpoint:".dimmed(), method.bold(), path.bold(), function.cyan().bold());
    println!("  {}  {}  {}",
        "request: ".dimmed(), trunc(&trace_id, 12).yellow(), ts.dimmed());
    println!("  {}  {}ms  \u{00b7}  {}  \u{00b7}  {} steps{}",
        "summary: ".dimmed(),
        total_ms,
        fmt_http_status(status),
        total_steps,
        if capped {
            format!("  \u{26a0}  truncated at 500 steps").red().to_string()
        } else { String::new() },
    );
    println!();

    // ── State + snapshot structures ───────────────────────────────────────────
    // Item 3: keyed by (table, pk) — tenant-scoped by API auth
    let mut state_at: HashMap<String, HashMap<String, Value>> = HashMap::new();
    // Item 8: one full snapshot per step for instant --at / prev in interactive mode
    let mut state_snapshots: Vec<HashMap<String, HashMap<String, Value>>> =
        Vec::with_capacity(total_steps);

    // Item 16: track previous start_time to detect parallel spans
    let mut prev_start = String::new();

    for (i, step) in steps.iter().enumerate() {
        let step_num  = i + 1;
        let step_muts = get_step_muts(i, step, has_span_ids, &mut_by_span, &legacy_map);

        apply_mutations(&step_muts, &mut state_at);
        state_snapshots.push(state_at.clone());  // item 8

        // Skip rendering when --at is set and we have not reached target yet
        if let Some(target) = at_step {
            if step_num < target { prev_start = step.start_time.clone(); continue; }
            if step_num > target { break; }
        }

        // Item 16: parallel span detection
        let is_parallel = !step.start_time.is_empty()
            && !prev_start.is_empty()
            && step.start_time.as_str() <= prev_start.as_str()
            && step.parent_id
                == steps.get(i.saturating_sub(1)).and_then(|p| p.parent_id.clone());
        prev_start = step.start_time.clone();

        // ── Step header ───────────────────────────────────────────────────────
        let div = "\u{2500}".repeat(tw.min(58)).dimmed();

        // Item 14 + 4: name red on error status
        let name_disp = if is_error_status(&step.status) {
            step.name.red().bold().to_string()
        } else {
            step.name.cyan().bold().to_string()
        };
        let ms_disp  = if step.duration_ms > 0 {
            format!("{}ms", step.duration_ms).dimmed().to_string()
        } else { String::new() };
        // Item 15: abbreviated span_id for log correlation
        let sid_disp = if !step.span_id.is_empty() {
            format!("  [{}]", trunc(&step.span_id, 8)).dimmed().to_string()
        } else { String::new() };
        let par_disp = if is_parallel { "  (parallel)".yellow().to_string() } else { String::new() };
        let st_disp  = fmt_span_status(&step.status);

        println!("{}", div);
        println!(" {}  \u{00b7}  {}    {}  {}{}{}",
            format!("Step {step_num} of {total_steps}").bold(),
            name_disp, ms_disp, st_disp, sid_disp, par_disp,
        );

        // Item 12: proportional timing bar (max 30 chars wide)
        if step.duration_ms > 0 {
            let bar_w = ((step.duration_ms as f64 / max_ms as f64) * 30.0).ceil() as usize;
            println!("    {}", "\u{2588}".repeat(bar_w.max(1)).dimmed());
        }

        // Item 5: tool / db call details
        if let Some(t) = &step.tool  { println!("    {}  {}", "tool: ".dimmed(),  t); }
        if let Some(t) = &step.table { println!("    {}  {}", "table:".dimmed(), t); }
        if let Some(q) = &step.query { println!("    {}  {}", "query:".dimmed(), trunc(q, tw.min(80))); }

        // Item 9: warn when mutation recorded outside a db span
        if !step_muts.is_empty() && step.span_type != "db" {
            println!("    {}",
                "\u{26a0}  mutation outside a db span \u{2014} possible instrumentation gap".yellow());
        }

        println!("{}", div);

        // ── Mutations ─────────────────────────────────────────────────────────
        if step_muts.is_empty() {
            println!("  {}", "no state changes in this step".dimmed());
        } else {
            for m in &step_muts {
                let table  = gstr(m, "table_name");
                let pk_key = pk_str(&m["record_pk"]);
                let op     = gstr(m, "operation");
                println!();
                println!("  {} {}.{}", color_op_prefix(op), table.cyan(), pk_key.yellow());
                match op {
                    "insert" => print_json_fields(&m["after_state"],  4),
                    "update" => print_update_diff(&m["before_state"], &m["after_state"], 4),
                    "delete" => print_json_fields(&m["before_state"], 4),
                    _ => {}
                }
            }
        }
        println!();

        // ── Interactive mode (item 7) ─────────────────────────────────────────
        if interactive {
            print!("  {}  > ", "[Enter] next  [s] state  [p] prev  [q] quit".dimmed());
            io::Write::flush(&mut io::stdout())?;
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            match line.trim() {
                "q" | "Q" => {
                    println!("  {}", "Exiting debugger.".dimmed());
                    return Ok(());
                }
                "s" | "S" => {
                    let snap = state_snapshots.last().unwrap_or(&state_at);
                    println!();
                    println!(" {}", format!("State after step {step_num}").bold());
                    println!("{}", div);
                    print_cumulative_state(snap);
                    println!();
                }
                "p" | "P" => {
                    if i > 0 {
                        let prev_snap = &state_snapshots[i - 1];
                        println!();
                        println!(" {}", format!("State after step {}", i).bold());
                        println!("{}", div);
                        print_cumulative_state(prev_snap);
                        println!();
                    } else {
                        println!("  {}", "Already at first step.".dimmed());
                    }
                }
                _ => {}
            }
        }

        // ── --at final snapshot (item 8) ──────────────────────────────────────
        if let Some(target) = at_step {
            if step_num == target {
                let div2 = "\u{2500}".repeat(tw.min(58)).dimmed();
                println!("{}", div2);
                println!(" {}", format!("State after step {step_num} ({})", step.name).bold());
                println!("{}", div2);
                print_cumulative_state(state_snapshots.last().unwrap_or(&state_at));
                println!();
                return Ok(());
            }
        }
    }

    // ── Final state ───────────────────────────────────────────────────────────
    if at_step.is_none() && !interactive {
        let div = "\u{2500}".repeat(tw.min(58)).dimmed();
        println!("{}", div);
        println!(" {}", format!("Final state after request {}", trunc(&trace_id, 12)).bold());
        println!("{}", div);
        print_cumulative_state(&state_at);
        println!();
    }

    Ok(())
}

// ── Mutation index lookup (item 11) ──────────────────────────────────────────

fn get_step_muts<'a>(
    i:            usize,
    step:         &Step,
    has_span_ids: bool,
    by_span:      &'a HashMap<String, Vec<&'a Value>>,
    legacy:       &'a [Vec<&'a Value>],
) -> Vec<&'a Value> {
    if has_span_ids {
        by_span.get(&step.span_id).cloned().unwrap_or_default()
    } else {
        legacy.get(i).cloned().unwrap_or_default()
    }
}

// ── State accumulation ────────────────────────────────────────────────────────

fn apply_mutations(
    muts:     &[&Value],
    state_at: &mut HashMap<String, HashMap<String, Value>>,
) {
    for m in muts {
        let table  = gstr(m, "table_name").to_string();
        let pk_key = pk_str(&m["record_pk"]);
        let op     = gstr(m, "operation");
        let entry  = state_at.entry(table).or_default();
        match op {
            "insert" | "update" => {
                if let Some(after) = m["after_state"].as_object() {
                    entry.insert(pk_key, Value::Object(after.clone()));
                }
            }
            "delete" => { entry.remove(&pk_key); }
            _ => {}
        }
    }
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn print_cumulative_state(state: &HashMap<String, HashMap<String, Value>>) {
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

/// Item 6: two-line field diff — each changed field on its own labelled block.
fn print_update_diff(before: &Value, after: &Value, indent: usize) {
    let pad  = " ".repeat(indent);
    let pad2 = " ".repeat(indent + 2);
    match (before.as_object(), after.as_object()) {
        (Some(b), Some(a)) => {
            let mut printed = false;
            for (k, av) in a {
                let bv = b.get(k).unwrap_or(&Value::Null);
                if bv != av {
                    println!("{pad}{}:", k.dimmed());
                    println!("{pad2}{} \u{2192} {}",
                        scalar_str(bv).yellow(), scalar_str(av).green());
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

fn print_json_fields(v: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            println!("{pad}{}: {}", k.dimmed(), scalar_str(val));
        }
    }
}

// ── Span helpers ──────────────────────────────────────────────────────────────

fn span_label(span: &Value) -> String {
    let kind = gstr(span, "span_type");
    span["data"]["action"].as_str()
        .or_else(|| span["data"]["tool"].as_str())
        .or_else(|| span["data"]["table"].as_str())
        .or_else(|| span["message"].as_str())
        .or_else(|| span["name"].as_str())
        .unwrap_or(kind)
        .to_string()
}

fn pk_str(pk: &Value) -> String {
    match pk {
        Value::Object(m) => {
            if let Some(id) = m.get("id") { return format!("id={}", scalar_str(id)); }
            m.iter().map(|(k, v)| format!("{k}={}", scalar_str(v))).collect::<Vec<_>>().join(",")
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
        _        => "\u{00b7}".normal(),
    }
}

fn fmt_http_status(status: i64) -> String {
    if status == 0       { "?".dimmed().to_string() }
    else if status < 400 { format!("{status} OK").green().to_string() }
    else                 { format!("{status} FAILED").red().to_string() }
}

fn fmt_span_status(status: &str) -> colored::ColoredString {
    if is_error_status(status)                              { status.red().bold() }
    else if status.is_empty() || status == "ok"
         || status == "success"                             { "ok".dimmed() }
    else                                                    { status.dimmed() }
}

fn is_error_status(s: &str) -> bool {
    matches!(s, "error" | "timeout" | "failed" | "failure")
}

// ── Safe field access (item 19) ───────────────────────────────────────────────

#[inline]
fn gstr<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

// ── Terminal width (item 18) ──────────────────────────────────────────────────

fn term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(80)
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}\u{2026}", &s[..n]) } else { s.to_string() }
}

async fn fetch_json(client: &ApiClient, url: &str) -> anyhow::Result<Value> {
    let res = client.client.get(url).send().await?;
    if res.status().is_success() {
        Ok(res.json().await.unwrap_or_default())
    } else { Ok(Value::Null) }
}
