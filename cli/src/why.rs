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

/// Compare two JSON objects and return changed fields as `(key, old_display, new_display)`.
/// Ignores `updated_at` / `created_at` unless they are the *only* change.
pub fn diff_json(before: &Value, after: &Value) -> Vec<(String, String, String)> {
    let mut diffs = Vec::new();
    let skip_set = ["updated_at", "created_at", "modified_at"];

    // Keys in after (new/changed values)
    if let (Some(bmap), Some(amap)) = (before.as_object(), after.as_object()) {
        for (key, aval) in amap {
            let bval = bmap.get(key).unwrap_or(&Value::Null);
            if bval != aval {
                if skip_set.contains(&key.as_str()) {
                    continue; // defer timestamp-only changes
                }
                diffs.push((key.clone(), json_scalar(bval), json_scalar(aval)));
            }
        }
        // Keys removed (present in before but not after)
        for key in bmap.keys() {
            if !amap.contains_key(key) && !skip_set.contains(&key.as_str()) {
                diffs.push((key.clone(), json_scalar(bmap.get(key).unwrap()), "∅".to_string()));
            }
        }
        // If nothing meaningful changed, fall back to timestamps
        if diffs.is_empty() {
            for (key, aval) in amap {
                let bval = bmap.get(key).unwrap_or(&Value::Null);
                if bval != aval {
                    diffs.push((key.clone(), json_scalar(bval), json_scalar(aval)));
                }
            }
        }
    }
    diffs
}

pub fn json_scalar(v: &Value) -> String {
    match v {
        Value::Null             => "∅".to_string(),
        Value::Bool(b)          => b.to_string(),
        Value::Number(n)        => n.to_string(),
        Value::String(s)        => trunc(s, 40),
        Value::Array(a)         => format!("[{}]", a.len()),
        Value::Object(_)        => "{…}".to_string(),
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

    // ── Fetch previous request (context-aware debugging) ─────────────────────
    // Find the request that ran immediately before this one.  We pass the first
    // span's timestamp as `before` so the API returns requests older than this one.
    let first_span_ts = spans.first()
        .and_then(|s| s["timestamp"].as_str())
        .unwrap_or("")
        .to_string();

    let prev_req: Option<Value> = if !first_span_ts.is_empty() {
        let prev_url = format!(
            "{}/traces?before={}&limit=1&exclude={}",
            client.base_url,
            urlencoding::encode(&first_span_ts),
            urlencoding::encode(&request_id),
        );
        let prev_res = client.client.get(&prev_url).send().await;
        if let Ok(res) = prev_res {
            if res.status().is_success() {
                let body: Value = res.json().await.unwrap_or_default();
                body["traces"].as_array().and_then(|arr| arr.first()).cloned()
            } else { None }
        } else { None }
    } else { None };

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

    // ── Execution graph ───────────────────────────────────────────────────────
    // Condense spans into a timeline table — source / resource / duration / slow mark.
    // Only includes spans that represent meaningful work (skips raw log entries).
    {
        let graph_types = [
            "request", "gateway_request", "http_request",
            "function", "db", "db_query",
            "tool", "workflow_step", "agent_step",
        ];
        let mut graph_spans: Vec<&Value> = spans.iter()
            .filter(|s| {
                let st = s["span_type"].as_str().unwrap_or("");
                graph_types.contains(&st)
            })
            .collect();

        // Sort by start offset: elapsed_ms - delta_ms
        graph_spans.sort_by_key(|s| {
            let elapsed = s["elapsed_ms"].as_i64().unwrap_or(0);
            let delta   = s["delta_ms"].as_i64().unwrap_or(0);
            elapsed - delta
        });
        graph_spans.truncate(12);

        if !graph_spans.is_empty() {
            println!();
            println!(
                "{}",
                "─── Execution graph ────────────────────────────────────────────────────────".dimmed()
            );
            for s in &graph_spans {
                let src = s["source"].as_str().unwrap_or("?");
                let res = s["resource"].as_str().unwrap_or("?");
                let ms  = s["delta_ms"].as_i64()
                    .filter(|&v| v > 0)
                    .unwrap_or_else(|| s["elapsed_ms"].as_i64().unwrap_or(0));
                let is_s = s["is_slow"].as_bool().unwrap_or(false) || ms > 500;

                let ms_str = if ms >= 1_000 {
                    format!("{:.1}s", ms as f64 / 1_000.0)
                } else {
                    format!("{}ms", ms)
                };

                let slow_suffix = if is_s { "  ⚠ slow".yellow().to_string() } else { String::new() };

                println!(
                    "  {}  {}  {}{}",
                    format!("{:<10}", src).dimmed(),
                    format!("{:<32}", trunc(res, 32)),
                    if is_s { ms_str.yellow().to_string() } else { ms_str.dimmed().to_string() },
                    slow_suffix,
                );
            }
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

            // Field-level diff for updates (skip insert/delete — show all fields or nothing)
            if op == "update" {
                let before = &m["before_state"];
                let after  = &m["after_state"];
                if before.is_object() && after.is_object() {
                    let diffs = diff_json(before, after);
                    let show = diffs.iter().take(6);
                    for (key, old_val, new_val) in show {
                        println!(
                            "      {}  {}  {}",
                            format!("{key}:").dimmed(),
                            old_val.red().strikethrough(),
                            format!("→ {new_val}").green(),
                        );
                    }
                    if diffs.len() > 6 {
                        println!("      {} more field{}", (diffs.len() - 6), if diffs.len() - 6 == 1 { "" } else { "s" });
                    }
                }
            } else if op == "insert" {
                // Show up to 3 notable fields for inserts
                if let Some(obj) = m["after_state"].as_object() {
                    let skip = ["id", "created_at", "updated_at", "tenant_id", "project_id"];
                    let notable: Vec<_> = obj.iter()
                        .filter(|(k, _)| !skip.contains(&k.as_str()))
                        .take(3)
                        .collect();
                    for (key, val) in notable {
                        println!(
                            "      {}  {}",
                            format!("{key}:").dimmed(),
                            json_scalar(val).green(),
                        );
                    }
                }
            }
        }
    }

    // ── Previous request ──────────────────────────────────────────────────────
    if let Some(prev) = &prev_req {
        let p_id     = prev["request_id"].as_str().unwrap_or("?");
        let p_method = prev["method"].as_str().unwrap_or("?");
        let p_path   = prev["path"].as_str().unwrap_or("?");
        let p_ms     = prev["duration_ms"].as_i64().unwrap_or(0);
        let p_status = prev["status"].as_i64().unwrap_or(0);
        let p_icon = match p_status {
            200..=299 => "✔".green().bold(),
            400..=499 => "!".yellow().bold(),
            _         => "✗".red().bold(),
        };

        // How long before this request did the previous one start?
        let gap_label: Option<String> = prev["started_at"].as_str().and_then(|prev_ts| {
            let t0 = chrono::DateTime::parse_from_rfc3339(&first_span_ts).ok()?;
            let t1 = chrono::DateTime::parse_from_rfc3339(prev_ts).ok()?;
            let ms = (t0 - t1).num_milliseconds();
            if ms <= 0 { return None; }
            Some(if ms < 1_000 {
                format!("{}ms before", ms)
            } else {
                format!("{:.1}s before", ms as f64 / 1000.0)
            })
        });

        // Rows that this request mutated — check if the previous request touched any.
        let mutated_rows: Vec<String> = mutations.iter().filter_map(|m| {
            let table = m["table_name"].as_str()?;
            let row_id = m["after_state"].get("id")
                .or_else(|| m["before_state"].get("id"))
                .and_then(|v| {
                    v.as_str().map(|s| trunc(s, 8))
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })?;
            Some(format!("{}.id={}", table, row_id))
        }).collect();

        println!(
            "{}",
            "─── Previous request ─────────────────────────────────────────────────────".dimmed()
        );
        let gap_suffix = gap_label
            .map(|g| format!("  ({})", g))
            .unwrap_or_default();
        println!(
            "  {} {}  {} {}  {}ms{}",
            p_icon,
            trunc(p_id, 12).dimmed(),
            p_method.bold(),
            trunc(p_path, 40),
            p_ms,
            gap_suffix.dimmed(),
        );
        for row in &mutated_rows {
            println!(
                "  {}  {}",
                "⚠ also modified".yellow(),
                row.yellow(),
            );
        }
        println!();
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
