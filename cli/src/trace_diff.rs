//! `flux trace diff <original-id> <replay-id>` — compare two executions of the same request.
//!
//! Shows how behavior diverged between the original production run and a replay:
//! duration, status, error presence, and state mutation field diffs.
//!
//! ```text
//! $ flux trace diff 9624a58d 550e8400
//!
//! ─── DIFF SUMMARY ────────────────────────────────────────────────
//!
//!   endpoint:   POST /signup → create_user
//!   original:   9624a58d    commit a93f42c
//!   replay:     550e8400    commit a93f42c   (replay:9624a58d)
//!
//! ─── Runtime ─────────────────────────────────────────────────────
//!   status:    500 FAILED → 200 OK            ✔ fixed
//!   duration:  3816ms → 142ms                 ↓ 96%
//!   errors:    1 → 0
//!
//! ─── State changes ───────────────────────────────────────────────
//!   users  v1  INSERT  (same)
//!
//!   users  v2  UPDATE  id=42
//!     plan:    free ─ original           replay: free → enterprise
//!
//! ─── Verdict ─────────────────────────────────────────────────────
//!   BEHAVIOR CHANGED  — state mutations differ
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
    let orig_errors = orig_trace["spans"].as_array().unwrap_or(&empty_spans)
        .iter().filter(|s| s["span_type"] == "error").count();
    let rep_errors  = rep_trace["spans"].as_array().unwrap_or(&empty_spans)
        .iter().filter(|s| s["span_type"] == "error").count();

    // ── Print header ─────────────────────────────────────────────────────────
    println!();
    println!("{}", "─── DIFF SUMMARY ────────────────────────────────────────────────────────────".bold());
    println!();
    println!("  {}  {} {} → {}", "endpoint:".dimmed(), method.bold(), path.bold(), function.cyan().bold());
    println!("  {}  {}  commit {}", "original:".dimmed(), trunc(&original_id, 12).yellow(), orig_commit.dimmed());
    println!("  {}  {}  commit {}", "replay:  ".dimmed(), trunc(&replay_id, 12).cyan(),    rep_commit.dimmed());
    println!();

    // ── Runtime section ──────────────────────────────────────────────────────
    println!("{}", "─── Runtime ─────────────────────────────────────────────────────────────────".bold());

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
    println!("  {}  {} {} → {}  {} {}",
        "status:  ".dimmed(),
        format_status(orig_status).yellow(),
        "→".dimmed(),
        format_status(rep_status).cyan(),
        status_icon,
        verdict_label,
    );

    // Duration
    let ms_pct = if orig_ms > 0 {
        let delta = rep_ms - orig_ms;
        let pct   = (delta * 100) / orig_ms;
        if delta < 0 { format!("↓ {}%", -pct).green().to_string() }
        else if delta == 0 { "(same)".dimmed().to_string() }
        else               { format!("↑ {}%", pct).red().to_string() }
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

    // ── State changes ────────────────────────────────────────────────────────
    let empty_arr = vec![];
    let orig_muts_arr = orig_muts["mutations"].as_array().unwrap_or(&empty_arr);
    let rep_muts_arr  = rep_muts ["mutations"].as_array().unwrap_or(&empty_arr);

    println!("{}", "─── State changes ───────────────────────────────────────────────────────────".bold());

    if orig_muts_arr.is_empty() && rep_muts_arr.is_empty() {
        println!("  {}", "no mutations in either execution".dimmed());
    } else {
        // Walk original mutations; match with replay mutations by (table, pk, op)
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
                    println!("  {}  v{}  {}{}",
                        table.cyan(),
                        ver,
                        color_op(op),
                        same_label,
                    );

                    if !row_id.is_empty() {
                        println!("  {}", format!("  id={row_id}").dimmed());
                    }

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
                                (Some((_, ob, oa)), Some((_, rb, ra))) if oa == ra => {
                                    // Identical change
                                    println!("    {}  {} → {}  (same)",
                                        format!("{key}:").dimmed(),
                                        ob.yellow(), ra.green());
                                }
                                (Some((_, ob, oa)), Some((_, _, ra))) => {
                                    // Different result
                                    println!("    {}  original {} → {}   replay → {}",
                                        format!("{key}:").dimmed(),
                                        ob.yellow(),
                                        oa.red(),
                                        ra.green().bold(),
                                    );
                                }
                                (Some((_, ob, oa)), None) => {
                                    println!("    {}  {} → {}  replay: {}",
                                        format!("{key}:").dimmed(),
                                        ob.yellow(), oa.red(),
                                        "(no change)".dimmed());
                                }
                                (None, Some((_, _, ra))) => {
                                    println!("    {}  original: {}  replay → {}",
                                        format!("{key}:").dimmed(),
                                        "(no change)".dimmed(), ra.green());
                                }
                                _ => {}
                            }
                        }
                    } else if !mutations_match && op == "insert" {
                        // Show notable field diffs for inserts
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
    println!("{}", "─── Verdict ─────────────────────────────────────────────────────────────────".bold());

    let behavior_changed = status_changed
        || orig_errors != rep_errors
        || orig_muts_arr.len() != rep_muts_arr.len()
        || orig_muts_arr.iter().zip(rep_muts_arr.iter())
            .any(|(o, r)| o["after_state"] != r["after_state"]);

    if behavior_changed {
        let (icon, label) = if !is_error(rep_status) && is_error(orig_status) {
            ("✔".green().bold().to_string(), "FIXED — replay succeeded".green().bold().to_string())
        } else if is_error(rep_status) && !is_error(orig_status) {
            ("✗".red().bold().to_string(), "REGRESSED — replay introduced error".red().bold().to_string())
        } else {
            ("≠".yellow().bold().to_string(), "BEHAVIOR CHANGED — state mutations differ".yellow().bold().to_string())
        };
        println!("  {} {}", icon, label);
    } else {
        println!("  {} {}", "═".dimmed(), "IDENTICAL — same state outcome in both executions".dimmed());
    }

    println!();
    Ok(())
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
