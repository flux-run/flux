//! `flux trace diff <original-id> <replay-id>` — compare two executions of the same request.
//!
//! Shows how behavior diverged between the original production run and a replay:
//! runtime metrics, per-span execution graph diff, and state mutation field diffs.
//!
//! ```text
//! $ flux trace diff 9624a58d a12b4f8c
//!
//! Runtime
//! ────────────────────────────
//! status:        error → success
//! duration:      3816ms → 142ms  (↓96%)
//! errors:        1 → 0
//!
//! Execution Graph
//! ────────────────────────────
//! stripe.charge
//!   original: timeout
//!   replay:   success
//!
//! gmail.send_email
//!   original: executed
//!   replay:   skipped
//!
//! State Diff
//! ────────────────────────────
//! users.id=42
//!
//!   plan
//!     original: free → pro
//!     replay:   free → enterprise
//!
//! Verdict
//! ────────────────────────────
//! FIXED
//! ```

use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use crate::why::{diff_json, json_scalar};

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn execute(
    original_id: String,
    replay_id:   String,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Fetch both traces + mutations concurrently
    let orig_trace_url = format!("{}/traces/{}?slow_ms=0", client.base_url, original_id);
    let rep_trace_url  = format!("{}/traces/{}?slow_ms=0", client.base_url, replay_id);
    let orig_mut_url   = format!("{}/db/mutations?request_id={}&limit=50", client.base_url, original_id);
    let rep_mut_url    = format!("{}/db/mutations?request_id={}&limit=50", client.base_url, replay_id);

    let (orig_trace, rep_trace, orig_muts, rep_muts) = tokio::try_join!(
        fetch_json(&client, &orig_trace_url),
        fetch_json(&client, &rep_trace_url),
        fetch_json(&client, &orig_mut_url),
        fetch_json(&client, &rep_mut_url),
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "original": { "trace": orig_trace, "mutations": orig_muts },
            "replay":   { "trace": rep_trace,  "mutations": rep_muts  },
        }))?);
        return Ok(());
    }

    // ── Extract metadata ─────────────────────────────────────────────────────
    let orig_req = orig_trace.get("request").unwrap_or(&Value::Null);
    let rep_req  = rep_trace .get("request").unwrap_or(&Value::Null);

    let method   = orig_req["method"].as_str().unwrap_or("?");
    let path     = orig_req["path"].as_str().unwrap_or("?");
    let function = orig_req["function"].as_str().unwrap_or("?");

    let orig_commit = short_sha(orig_req["code_sha"].as_str().unwrap_or("?"));
    let rep_commit  = short_sha(rep_req ["code_sha"].as_str().unwrap_or("?"));

    let orig_status  = orig_req["status"].as_i64().unwrap_or(0);
    let rep_status   = rep_req ["status"].as_i64().unwrap_or(0);

    let orig_ms = orig_trace["total_ms"].as_i64().unwrap_or(0);
    let rep_ms  = rep_trace ["total_ms"].as_i64().unwrap_or(0);

    let empty_spans: Vec<Value> = vec![];
    let orig_spans_all = orig_trace["spans"].as_array().unwrap_or(&empty_spans);
    let rep_spans_all  = rep_trace["spans"].as_array().unwrap_or(&empty_spans);

    let orig_errors = orig_spans_all.iter().filter(|s| s["span_type"] == "error").count();
    let rep_errors  = rep_spans_all .iter().filter(|s| s["span_type"] == "error").count();

    // ── Print header ─────────────────────────────────────────────────────────
    println!();
    println!("  {}  {} {} → {}", "endpoint:".dimmed(), method.bold(), path.bold(), function.cyan().bold());
    println!("  {}  {}  commit {}", "original:".dimmed(), trunc(&original_id, 12).yellow(), orig_commit.dimmed());
    println!("  {}  {}  commit {}", "replay:  ".dimmed(), trunc(&replay_id, 12).cyan(),    rep_commit.dimmed());
    println!();

    // ── Runtime section ──────────────────────────────────────────────────────
    println!("{}", "Runtime".bold());
    println!("{}", "─".repeat(28).dimmed());

    // Status
    let status_changed = orig_status != rep_status;
    let status_icon = if !status_changed {
        "═".dimmed().to_string()
    } else if is_error(rep_status) {
        "✗".red().bold().to_string()
    } else {
        "✔".green().bold().to_string()
    };
    let verdict_label = if !status_changed {
        "(same)".dimmed().to_string()
    } else if is_error(orig_status) && !is_error(rep_status) {
        "fixed".green().bold().to_string()
    } else if !is_error(orig_status) && is_error(rep_status) {
        "regressed".red().bold().to_string()
    } else {
        "changed".yellow().to_string()
    };
    println!("  {}  {} → {}  {} {}",
        "status:  ".dimmed(),
        format_status(orig_status).yellow(),
        format_status(rep_status).cyan(),
        status_icon,
        verdict_label,
    );

    // Duration
    let ms_pct = if orig_ms > 0 {
        let delta = rep_ms - orig_ms;
        let pct   = (delta * 100) / orig_ms;
        if delta < 0 { format!("(↓{}%)", -pct).green().to_string() }
        else if delta == 0 { "(same)".dimmed().to_string() }
        else               { format!("(↑{}%)", pct).red().to_string() }
    } else {
        String::new()
    };
    println!("  {}  {}ms → {}ms  {}",
        "duration:".dimmed(),
        orig_ms.to_string().yellow(),
        rep_ms.to_string().cyan(),
        ms_pct,
    );

    // Errors
    let err_icon = if orig_errors > rep_errors {
        format!("↓ {}", orig_errors - rep_errors).green().to_string()
    } else if rep_errors > orig_errors {
        format!("↑ {}", rep_errors - orig_errors).red().to_string()
    } else {
        "(same)".dimmed().to_string()
    };
    println!("  {}  {} → {}  {}",
        "errors:  ".dimmed(),
        orig_errors.to_string().yellow(),
        rep_errors.to_string().cyan(),
        err_icon,
    );
    println!();

    // ── Execution Graph section ───────────────────────────────────────────────
    let span_diffs = diff_spans(orig_spans_all, rep_spans_all);
    let any_span_diff = span_diffs.iter().any(|d| d.changed);

    if any_span_diff {
        println!("{}", "Execution Graph".bold());
        println!("{}", "─".repeat(28).dimmed());

        for sd in span_diffs.iter().filter(|d| d.changed) {
            println!();
            println!("  {}", sd.name.cyan().bold());
            match (&sd.orig_status, &sd.rep_status) {
                (Some(o), Some(r)) => {
                    println!("  {}  {}", "  original:".dimmed(), color_status_label(o));
                    println!("  {}  {}", "  replay:  ".dimmed(), color_status_label(r));
                }
                (Some(o), None) => {
                    println!("  {}  {}", "  original:".dimmed(), color_status_label(o));
                    println!("  {}  {}", "  replay:  ".dimmed(), "skipped".dimmed());
                }
                (None, Some(r)) => {
                    println!("  {}  {}", "  original:".dimmed(), "skipped".dimmed());
                    println!("  {}  {}", "  replay:  ".dimmed(), color_status_label(r));
                }
                (None, None) => {}
            }
            // Duration change, if both ran and differed meaningfully
            if let (Some(od), Some(rd)) = (sd.orig_ms, sd.rep_ms) {
                if od > 0 {
                    let abs_diff = (rd - od).abs();
                    let delta_pct = (abs_diff * 100) / od;
                    if abs_diff >= 100 || delta_pct >= 20 {
                        let arrow = if delta_pct < 0 {
                            format!("↓{}%", -delta_pct).green().to_string()
                        } else {
                            format!("↑{}%", delta_pct).red().to_string()
                        };
                        println!("  {}  {}ms → {}ms  {}",
                            "  duration:".dimmed(), od, rd, arrow);
                    }
                }
            }
        }
        println!();
    }

    // ── State changes ────────────────────────────────────────────────────────
    let empty_arr = vec![];
    let orig_muts_arr = orig_muts["mutations"].as_array().unwrap_or(&empty_arr);
    let rep_muts_arr  = rep_muts ["mutations"].as_array().unwrap_or(&empty_arr);

    println!("{}", "State Diff".bold());
    println!("{}", "─".repeat(28).dimmed());

    if orig_muts_arr.is_empty() && rep_muts_arr.is_empty() {
        println!("  {}", "no mutations in either execution".dimmed());
    } else {
        // Walk original mutations; match with replay mutations by position
        let max = orig_muts_arr.len().max(rep_muts_arr.len());
        for i in 0..max {
            let om = orig_muts_arr.get(i);
            let rm = rep_muts_arr.get(i);

            match (om, rm) {
                (Some(o), Some(r)) => {
                    let table = o["table_name"].as_str().unwrap_or("?");
                    let op    = o["operation"].as_str().unwrap_or("?");
                    let ver   = o["version"].as_i64().unwrap_or(0);

                    let mutations_match = o["after_state"] == r["after_state"]
                        && o["before_state"] == r["before_state"];

                    let row_id = o["after_state"].get("id")
                        .or_else(|| o["before_state"].get("id"))
                        .and_then(|v| v.as_str().map(|s| trunc(s, 8))
                            .or_else(|| v.as_i64().map(|n| n.to_string())))
                        .unwrap_or_default();

                    let same_label = if mutations_match { "  (same)".dimmed().to_string() } else { String::new() };

                    println!();
                    println!("  {}.{}{}  {}{}",
                        table.cyan(),
                        if row_id.is_empty() { format!("v{ver}") } else { format!("id={row_id}") },
                        String::new(),
                        color_op(op),
                        same_label,
                    );

                    if !mutations_match && op == "update" {
                        // Side-by-side field diff
                        let o_diffs = diff_json(&o["before_state"], &o["after_state"]);
                        let r_diffs = diff_json(&r["before_state"], &r["after_state"]);

                        // All field keys involved in either
                        let mut keys: Vec<String> = o_diffs.iter().map(|(k,_,_)| k.clone())
                            .chain(r_diffs.iter().map(|(k,_,_)| k.clone()))
                            .collect();
                        keys.dedup();

                        for key in keys {
                            let o_change = o_diffs.iter().find(|(k,_,_)| k == &key);
                            let r_change = r_diffs.iter().find(|(k,_,_)| k == &key);

                            match (o_change, r_change) {
                                (Some((_, ob, oa)), Some((_, _rb, ra))) if oa == ra => {
                                    println!("    {}  {} → {}  (same)",
                                        format!("{key}:").dimmed(),
                                        ob.yellow(), ra.green());
                                }
                                (Some((_, ob, oa)), Some((_, rb, ra))) => {
                                    println!();
                                    println!("    {}", key.bold());
                                    println!("    {}  {} → {}",
                                        "  original:".dimmed(), ob.yellow(), oa.red());
                                    println!("    {}  {} → {}",
                                        "  replay:  ".dimmed(), rb.yellow(), ra.green().bold());
                                }
                                (Some((_, ob, oa)), None) => {
                                    println!();
                                    println!("    {}", key.bold());
                                    println!("    {}  {} → {}",
                                        "  original:".dimmed(), ob.yellow(), oa.red());
                                    println!("    {}  {}", "  replay:  ".dimmed(), "(no change)".dimmed());
                                }
                                (None, Some((_, _, ra))) => {
                                    println!();
                                    println!("    {}", key.bold());
                                    println!("    {}  {}", "  original:".dimmed(), "(no change)".dimmed());
                                    println!("    {}  → {}", "  replay:  ".dimmed(), ra.green().bold());
                                }
                                _ => {}
                            }
                        }
                    } else if !mutations_match && op == "insert" {
                        let diffs = diff_json(&r["after_state"], &o["after_state"]);
                        for (key, rval, oval) in diffs.iter().take(5) {
                            println!("    {}  original: {}   replay: {}",
                                format!("{key}:").dimmed(), oval.yellow(), rval.cyan());
                        }
                    }
                }
                (Some(o), None) => {
                    let table = o["table_name"].as_str().unwrap_or("?");
                    let op    = o["operation"].as_str().unwrap_or("?");
                    println!();
                    println!("  {}  {}  {}  replay: {}", table.cyan(), color_op(op), "v?".dimmed(), "missing".red());
                }
                (None, Some(r)) => {
                    let table = r["table_name"].as_str().unwrap_or("?");
                    let op    = r["operation"].as_str().unwrap_or("?");
                    println!();
                    println!("  {}  {}  {}  original: {}", table.cyan(), color_op(op), "v?".dimmed(), "missing".red());
                }
                (None, None) => {}
            }
        }
    }

    println!();

    // ── Verdict ──────────────────────────────────────────────────────────────
    println!("{}", "Verdict".bold());
    println!("{}", "─".repeat(28).dimmed());

    let state_changed = orig_muts_arr.len() != rep_muts_arr.len()
        || orig_muts_arr.iter().zip(rep_muts_arr.iter())
            .any(|(o, r)| o["after_state"] != r["after_state"]);

    let behavior_changed = status_changed || orig_errors != rep_errors || state_changed || any_span_diff;

    if behavior_changed {
        let (icon, label) = if !is_error(rep_status) && is_error(orig_status) {
            ("✔".green().bold().to_string(), "FIXED".green().bold().to_string())
        } else if is_error(rep_status) && !is_error(orig_status) {
            ("✗".red().bold().to_string(), "REGRESSED".red().bold().to_string())
        } else {
            ("≠".yellow().bold().to_string(), "BEHAVIOR CHANGED".yellow().bold().to_string())
        };
        println!("  {} {}", icon, label);
    } else {
        println!("  {} {}", "═".dimmed(), "IDENTICAL".dimmed());
    }

    println!();
    Ok(())
}

// ── Span diff ────────────────────────────────────────────────────────────────

/// Summary of a single span's observable behaviour across one execution.
#[derive(Debug, Default)]
struct SpanEntry {
    status: Option<String>,
    ms:     Option<i64>,
}

/// One row in the execution-graph diff table.
struct SpanDiff {
    name:        String,
    orig_status: Option<String>,
    rep_status:  Option<String>,
    orig_ms:     Option<i64>,
    rep_ms:      Option<i64>,
    /// True when the two executions differ in a user-visible way.
    changed:     bool,
}

/// Compare the span arrays from two traces.
/// Returns one `SpanDiff` per span name that appears in at least one trace,
/// ordered by first appearance in the original.
fn diff_spans(orig: &[Value], rep: &[Value]) -> Vec<SpanDiff> {
    fn relevant(s: &Value) -> bool {
        matches!(
            s["span_type"].as_str().unwrap_or(""),
            "tool" | "db" | "workflow_step" | "agent_step"
        )
    }

    fn span_key(s: &Value) -> String {
        let kind = s["span_type"].as_str().unwrap_or("span");
        // Prefer a human-readable name from common fields, fall back to span_type
        let name = s["data"]["action"].as_str()
            .or_else(|| s["data"]["tool"].as_str())
            .or_else(|| s["data"]["table"].as_str())
            .or_else(|| s["message"].as_str())
            .or_else(|| s["name"].as_str())
            .unwrap_or(kind);
        name.to_string()
    }

    fn span_status(s: &Value) -> Option<String> {
        s["status"].as_str()
            .or_else(|| if s["span_type"] == "error" { Some("error") } else { None })
            .or_else(|| if s["data"]["success"].as_bool() == Some(false) { Some("error") } else { None })
            .map(|s| s.to_string())
            .or_else(|| Some("executed".to_string()))
    }

    fn collect(spans: &[Value]) -> Vec<(String, SpanEntry)> {
        let mut vec: Vec<(String, SpanEntry)> = Vec::new();
        for s in spans.iter().filter(|s| relevant(s)) {
            let key = span_key(s);
            if let Some((_, e)) = vec.iter_mut().find(|(k, _)| k == &key) {
                e.status = span_status(s);
                e.ms = s["duration_ms"].as_i64().or(e.ms);
            } else {
                vec.push((key, SpanEntry {
                    status: span_status(s),
                    ms:     s["duration_ms"].as_i64(),
                }));
            }
        }
        vec
    }

    let orig_vec = collect(orig);
    let rep_vec  = collect(rep);

    // Union of all keys, original order first, then replay-only keys
    let mut all_keys: Vec<String> = orig_vec.iter().map(|(k, _)| k.clone()).collect();
    for (k, _) in &rep_vec {
        if !all_keys.contains(k) {
            all_keys.push(k.clone());
        }
    }

    all_keys.into_iter().map(|name| {
        let oe = orig_vec.iter().find(|(k, _)| k == &name).map(|(_, e)| e);
        let re = rep_vec .iter().find(|(k, _)| k == &name).map(|(_, e)| e);

        let orig_status = oe.and_then(|e| e.status.clone());
        let rep_status  = re.and_then(|e| e.status.clone());
        let orig_ms     = oe.and_then(|e| e.ms);
        let rep_ms      = re.and_then(|e| e.ms);

        // Status changed, or one side is absent (skipped vs executed)
        let status_diff = orig_status != rep_status || oe.is_none() != re.is_none();

        // Duration changed by >= 100ms absolute OR >= 20% relative.
        // The absolute guard prevents small spans (10ms → 20ms) from appearing as noise.
        let dur_diff = match (orig_ms, rep_ms) {
            (Some(o), Some(r)) if o > 0 => {
                let abs_diff = (r - o).abs();
                let pct_diff = (abs_diff * 100) / o;
                abs_diff >= 100 || pct_diff >= 20
            }
            _ => false,
        };

        SpanDiff {
            name,
            orig_status,
            rep_status,
            orig_ms,
            rep_ms,
            changed: status_diff || dur_diff,
        }
    }).collect()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn fetch_json(client: &ApiClient, url: &str) -> anyhow::Result<Value> {
    let res = client.client.get(url).send().await?;
    if res.status().is_success() {
        Ok(res.json().await.unwrap_or_default())
    } else {
        Ok(Value::Null)
    }
}

fn color_status_label(s: &str) -> colored::ColoredString {
    match s {
        "error" | "timeout" | "failed" => s.red().bold(),
        "success" | "executed"         => s.green(),
        "skipped"                      => s.dimmed(),
        _                              => s.normal(),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}…", &s[..n]) } else { s.to_string() }
}

fn short_sha(sha: &str) -> String {
    if sha.len() >= 7 { sha[..7].to_string() } else { sha.to_string() }
}

fn is_error(status: i64) -> bool { status >= 400 || status == 0 }

fn format_status(status: i64) -> String {
    if status == 0 { "?".to_string() }
    else if status < 400 { format!("{status} OK") }
    else { format!("{status} FAILED") }
}

fn color_op(op: &str) -> colored::ColoredString {
    match op {
        "insert" => op.green().bold(),
        "update" => op.yellow().bold(),
        "delete" => op.red().bold(),
        _        => op.normal(),
    }
}
