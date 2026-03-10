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
use std::collections::HashSet;
use tokio::time::{sleep, Duration};

use crate::client::ApiClient;

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
        println!("{}", "Fluxbase · Live Request Stream".bold());
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
    // "since" cursor — server-side ISO timestamp filter
    let mut since: Option<String> = Some(chrono::Utc::now().to_rfc3339());

    loop {
        let mut url = format!("{}/traces?limit=25&order=desc", client.base_url);
        if let Some(f) = &function {
            url.push_str(&format!("&function={}", f));
        }
        if let Some(ts) = &since {
            url.push_str(&format!("&since={}", ts));
        }

        if let Ok(res) = client.client.get(&url).send().await {
            if let Ok(body) = res.json::<Value>().await {
                let empty = vec![];
                let rows = body
                    .get("data")
                    .and_then(|d| d.as_array())
                    .unwrap_or(&empty);

                // Newest first from server; print in arrival order (oldest first)
                let mut new_rows: Vec<&Value> = rows
                    .iter()
                    .filter(|r| {
                        let id = r["request_id"].as_str().unwrap_or("");
                        !id.is_empty() && !seen_ids.contains(id)
                    })
                    .collect();

                // Reverse so we print oldest of the new batch first
                new_rows.reverse();

                for row in &new_rows {
                    let id = row["request_id"].as_str().unwrap_or("").to_string();
                    let method = row["method"].as_str().unwrap_or("?");
                    let route = row["route"].as_str().unwrap_or("?");
                    let func = row["function"].as_str().unwrap_or("?");
                    let duration_ms = row["total_ms"].as_i64().unwrap_or(0);
                    let status = row["status"].as_str().unwrap_or("?");
                    let is_error = status == "error" || status == "failed";
                    let is_slow = slow_threshold
                        .map(|t| duration_ms as u64 > t)
                        .unwrap_or(false);

                    // Apply filters
                    if errors_only && !is_error {
                        seen_ids.insert(id);
                        continue;
                    }
                    if slow_threshold.is_some() && !is_slow && !is_error {
                        seen_ids.insert(id);
                        continue;
                    }

                    if json_output {
                        println!("{}", serde_json::to_string(row)?);
                    } else {
                        let dur_str = fmt_duration(duration_ms);
                        let status_str = if is_error {
                            format!("✗ {}", status).red().bold().to_string()
                        } else if is_slow {
                            "✔".yellow().bold().to_string()
                        } else {
                            "✔".green().bold().to_string()
                        };

                        let method_col = fmt_method(method);

                        let error_hint = if is_error {
                            let err = row["error"].as_str().unwrap_or("");
                            if !err.is_empty() {
                                // Clickable instruction on next line
                                format!(
                                    "\n   {} {}",
                                    "→".dimmed(),
                                    format!("flux debug {}", id).cyan()
                                )
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };

                        println!(
                            "{}  {}  {}  {}  {}{}",
                            method_col,
                            trunc(route, 28),
                            trunc(func, 22).dimmed(),
                            dur_str,
                            status_str,
                            error_hint
                        );

                        // auto-debug: pause the stream and run the full debugger
                        if auto_debug && is_error && !id.is_empty() {
                            println!();
                            println!(
                                "{} {}",
                                "Opening debugger for".yellow().bold(),
                                id.cyan()
                            );
                            println!("{}", "─".repeat(72).dimmed());
                            if let Err(e) = crate::debug::execute_auto(id.clone()).await {
                                eprintln!("{} auto-debug failed: {}", "✗".red(), e);
                            }
                            println!();
                            println!("{}", "─".repeat(72).dimmed());
                            println!("{}", "Resuming live stream...".dimmed());
                            println!();
                        }
                    }

                    // Update "since" to the newest timestamp we've seen
                    if let Some(ts) = row["created_at"].as_str() {
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
