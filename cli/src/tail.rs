//! `flux tail [function]` — live request stream.
//!
//! Streams incoming requests in real time, like htop for your backend.
//! Each line shows method, route, function, duration, and pass/fail.
//!
//! ```text
//! flux tail                           # all functions
//! flux tail create_user               # single function
//! flux tail --errors                  # errors only
//! flux tail --slow 500                # requests > 500ms only
//! flux tail --json                    # machine-readable
//! flux tail --auto-debug              # auto-run `flux debug` when any error appears
//! ```

use colored::Colorize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tokio::time::{sleep, Duration};

use api_contract::routes as R;
use crate::client::ApiClient;
use crate::why::diff_json;

/// Fetch up to `max` mutation rows for a request and return short display strings.
/// Returns: Vec of (row_key like "users.id=7f3a", change_summary)
async fn fetch_mutations(client: &ApiClient, request_id: &str) -> Vec<(String, String)> {
    let url = format!("{}?request_id={}&limit=3", R::db::MUTATIONS.url(&client.base_url), request_id);
    let Ok(res) = client.client.get(&url).send().await else { return vec![]; };
    if !res.status().is_success() { return vec![]; }
    let Ok(body): Result<Value, _> = res.json().await else { return vec![]; };
    let muts = body["mutations"].as_array().cloned().unwrap_or_default();
    muts.iter().take(3).filter_map(|m| {
        let table = m["table_name"].as_str()?;
        let op    = m["operation"].as_str().unwrap_or("?");
        let row_id = m["after_state"].get("id")
            .or_else(|| m["before_state"].get("id"))
            .and_then(|v| v.as_str().map(|s| s[..s.len().min(8)].to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();
        let row_key = if row_id.is_empty() {
            table.to_string()
        } else {
            format!("{}.id={}", table, row_id)
        };
        let summary = match op {
            "insert" => "insert".to_string(),
            "delete" => "delete".to_string(),
            "update" => {
                let diffs = diff_json(&m["before_state"], &m["after_state"]);
                if diffs.is_empty() { "update".to_string() }
                else if diffs.len() == 1 {
                    format!("{} {} \u{2192} {}", diffs[0].0, diffs[0].1, diffs[0].2)
                } else {
                    let (k, old, new) = &diffs[0];
                    format!("{} {} \u{2192} {}  +{} more", k, old, new, diffs.len()-1)
                }
            }
            _ => op.to_string(),
        };
        Some((row_key, summary))
    }).collect()
}

pub async fn execute(
    function: Option<String>,
    errors_only: bool,
    slow_threshold: Option<u64>,
    json_output: bool,
    auto_debug: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    if !json_output {
        println!();
        println!("{}", "Flux · Live Request Stream".bold());
        let filter = match (&function, errors_only, slow_threshold, auto_debug) {
            (Some(f), _, _, _) => format!("function: {}", f.cyan()),
            (None, true, _, _) => "errors only".red().to_string(),
            (None, false, Some(ms), _) => format!("slow > {}ms", ms).yellow().to_string(),
            _ => "all requests".dimmed().to_string(),
        };
        let auto_badge = if auto_debug {
            format!("  {}", "auto-debug on".yellow())
        } else {
            String::new()
        };
        println!("{} {}{}", "Watching:".dimmed(), filter, auto_badge);
        println!("{}", "─".repeat(72).dimmed());
        println!(
            "{}  {}  {}  {}  {}",
            fmt_col("METHOD", 7),
            fmt_col("ROUTE", 28),
            fmt_col("FUNCTION", 22),
            fmt_col("DURATION", 10),
            "STATUS"
        );
        println!("{}", "─".repeat(72).dimmed());
    }

    let mut seen_ids: HashSet<String> = HashSet::new();
    // since cursor — forward ISO timestamp filter for list_traces
    let mut since: Option<String> = Some(chrono::Utc::now().to_rfc3339());
    // track last-seen mutated row key per table.pk for same-row warnings
    let mut last_mutated: HashMap<String, String> = HashMap::new();

    loop {
        let mut url = format!("{}?limit=20", R::logs::TRACES_LIST.url(&client.base_url));
        if let Some(f) = &function {
            url.push_str(&format!("&function={}", urlencoding::encode(f)));
        }
        if let Some(ts) = &since {
            url.push_str(&format!("&since={}", urlencoding::encode(ts)));
        }

        if let Ok(res) = client.client.get(&url).send().await {
            if let Ok(body) = res.json::<Value>().await {
                let empty = vec![];
                // list_traces returns { "traces": [...] }  (was wrongly "data")
                let rows = body
                    .get("traces")
                    .and_then(|d| d.as_array())
                    .unwrap_or(&empty);

                // list_traces returns oldest-first in since-mode; print as-is
                let new_rows: Vec<&Value> = rows
                    .iter()
                    .filter(|r| {
                        let id = r["request_id"].as_str().unwrap_or("");
                        !id.is_empty() && !seen_ids.contains(id)
                    })
                    .collect();

                // Only fetch mutations for error rows + first 2 healthy rows per cycle
                let mut healthy_fetched = 0u8;

                for row in &new_rows {
                    let id          = row["request_id"].as_str().unwrap_or("").to_string();
                    let method      = row["method"].as_str().unwrap_or("?");
                    // list_traces uses "path"; legacy fallback to "route"
                    let path        = row["path"].as_str()
                        .or_else(|| row["route"].as_str())
                        .unwrap_or("?");
                    let func        = row["function"].as_str().unwrap_or("");
                    let duration_ms = row["duration_ms"].as_i64()
                        .or_else(|| row["total_ms"].as_i64())
                        .unwrap_or(0);
                    let http_status = row["status"].as_i64().unwrap_or(0);
                    let is_error    = row["is_error"].as_bool()
                        .unwrap_or(http_status >= 400);
                    let error_msg   = row["error"].as_str().unwrap_or("");
                    let is_slow     = slow_threshold
                        .map(|t| duration_ms as u64 > t)
                        .unwrap_or(false);

                    // Apply filters
                    if errors_only && !is_error {
                        if let Some(ts) = row["started_at"].as_str() { since = Some(ts.to_string()); }
                        seen_ids.insert(id);
                        continue;
                    }
                    if slow_threshold.is_some() && !is_slow && !is_error {
                        if let Some(ts) = row["started_at"].as_str() { since = Some(ts.to_string()); }
                        seen_ids.insert(id);
                        continue;
                    }

                    if json_output {
                        println!("{}", serde_json::to_string(row)?);
                    } else {
                        let dur_str = fmt_duration(duration_ms);

                        let status_str = if is_error {
                            let code = if http_status > 0 {
                                http_status.to_string()
                            } else {
                                "ERR".to_string()
                            };
                            format!("✗ {}", code).red().bold().to_string()
                        } else if is_slow {
                            "✔".yellow().bold().to_string()
                        } else {
                            "✔".green().bold().to_string()
                        };

                        let func_col = if func.is_empty() {
                            trunc("?", 22).dimmed().to_string()
                        } else {
                            trunc(func, 22).dimmed().to_string()
                        };

                        println!(
                            "{}  {}  {}  {}  {}",
                            fmt_method(method),
                            trunc(path, 28),
                            func_col,
                            dur_str,
                            status_str,
                        );

                        // ── Inline error message ─────────────────────────────
                        if is_error && !error_msg.is_empty() {
                            let trunc_err = if error_msg.len() > 72 {
                                format!("{}…", &error_msg[..71])
                            } else {
                                error_msg.to_string()
                            };
                            println!("   {} {}", "error:".dimmed(), trunc_err.red());
                            println!("   {} {}",
                                "→".dimmed(),
                                format!("flux why {}", &id[..id.len().min(12)]).cyan(),
                            );
                        }

                        // ── Mutation summary ─────────────────────────────────
                        // Fetch for all errors + first 2 non-error rows per poll
                        let should_fetch = is_error || healthy_fetched < 2;
                        if should_fetch && !id.is_empty() {
                            if !is_error { healthy_fetched += 1; }
                            let muts = fetch_mutations(&client, &id).await;
                            let mut same_row_warned = false;
                            for (row_key, summary) in &muts {
                                // Same-row warning: did a previous request touch this row?
                                let warn = last_mutated.get(row_key)
                                    .filter(|prev_id| *prev_id != &id)
                                    .map(|prev_id| prev_id[..prev_id.len().min(8)].to_string());
                                if warn.is_some() && !same_row_warned {
                                    println!("   {}",
                                        format!("⚠ same row as previous request").yellow());
                                    same_row_warned = true;
                                }
                                println!("   {}  {}",
                                    row_key.cyan(),
                                    summary.dimmed(),
                                );
                                last_mutated.insert(row_key.clone(), id.clone());
                            }
                            // Keep last_mutated bounded
                            if last_mutated.len() > 200 { last_mutated.clear(); }
                        }

                    }

                    // Advance cursor to the latest started_at we've seen
                    if let Some(ts) = row["started_at"].as_str() {
                        since = Some(ts.to_string());
                    }
                    seen_ids.insert(id);
                }
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn fmt_col(s: &str, width: usize) -> String {
    format!("{:<width$}", s.bold())
}

fn fmt_method(method: &str) -> String {
    let padded = format!("{:<7}", method);
    match method.to_uppercase().as_str() {
        "GET" => padded.green().to_string(),
        "POST" => padded.cyan().to_string(),
        "PUT" | "PATCH" => padded.yellow().to_string(),
        "DELETE" => padded.red().to_string(),
        _ => padded.normal().to_string(),
    }
}

fn fmt_duration(ms: i64) -> String {
    let s = if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}ms", ms)
    };
    let padded = format!("{:<10}", s);
    if ms > 2000 {
        padded.red().to_string()
    } else if ms > 500 {
        padded.yellow().to_string()
    } else {
        padded.normal().to_string()
    }
}

fn trunc(s: &str, max: usize) -> String {
    if s.len() <= max {
        format!("{:<max$}", s)
    } else {
        format!("{:.prec$}…", s, prec = max - 1)
    }
}
