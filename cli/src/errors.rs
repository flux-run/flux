//! `flux errors` — production error summary by function.
//!
//! Shows per-function error counts, most recent error type, and p95 duration
//! across a configurable time window. Quick triage before running `flux debug`.
//!
//! ```text
//! flux errors                  # last 1h across all functions
//! flux errors --since 24h      # last 24h
//! flux errors --function create_user   # single function
//! flux errors --json           # machine-readable
//! ```

use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

pub async fn execute(
    function: Option<String>,
    since: String,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let mut url = format!(
        "{}/traces/errors/summary?since={}",
        client.base_url, since
    );
    if let Some(f) = &function {
        url.push_str(&format!("&function={}", f));
    }

    let res = client.client.get(&url).send().await?;
    let body: Value = res.json().await.unwrap_or_default();

    let empty = vec![];
    let entries: &Vec<Value> = body
        .get("data")
        .and_then(|d| d.as_array())
        .unwrap_or(&empty);

    if json_output {
        println!("{}", serde_json::to_string_pretty(entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!(
            "{}",
            format!("No errors in the last {}. Your backend is healthy! ✔", since)
                .green()
                .bold()
        );
        return Ok(());
    }

    println!();
    println!(
        "{} {}",
        "Production Errors".bold(),
        format!("(last {})", since).dimmed()
    );
    println!("{}", "─".repeat(52).dimmed());

    for entry in entries {
        let func = entry["function"].as_str().unwrap_or("?");
        let count = entry["error_count"].as_i64().unwrap_or(0);
        let last_error = entry["last_error"].as_str().unwrap_or("unknown");
        let p95 = entry["p95_ms"].as_i64();
        let last_at = entry["last_at"]
            .as_str()
            .map(|s| s.get(..16).unwrap_or(s).replace('T', " "))
            .unwrap_or_default();

        // Function name + error badge
        let count_badge = if count > 10 {
            format!("{} errors", count).red().bold().to_string()
        } else {
            format!("{} error{}", count, if count == 1 { "" } else { "s" })
                .yellow()
                .to_string()
        };

        println!("  {}  {}", func.bold(), count_badge);
        println!(
            "    {}  {}",
            "last:".dimmed(),
            last_error.red()
        );
        if let Some(ms) = p95 {
            let dur = if ms >= 1000 {
                format!("{:.1}s", ms as f64 / 1000.0)
            } else {
                format!("{}ms", ms)
            };
            println!("    {}  {}", "p95: ".dimmed(), dur.yellow());
        }
        if !last_at.is_empty() {
            println!("    {}  {}", "at:  ".dimmed(), last_at.dimmed());
        }

        // Quick-action hint
        println!(
            "    {} {}",
            "→".cyan(),
            format!("flux debug --function {}", func).cyan()
        );
        println!();
    }

    Ok(())
}
