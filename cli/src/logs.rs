use crate::client::ApiClient;
use colored::Colorize;
use serde_json::Value;

// ── Formatting helpers ────────────────────────────────────────────────────

fn format_timestamp(ts: &str) -> String {
    // Input: RFC-3339 like "2026-03-09T10:01:12.000000Z"
    // Output: "2026-03-09 10:01:12"
    ts.get(..19)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| ts.to_string())
}

fn colorize_level(level: &str) -> colored::ColoredString {
    match level.to_uppercase().as_str() {
        "ERROR" | "ERR" => level.to_uppercase().red().bold(),
        "WARN"  | "WARNING" => level.to_uppercase().yellow().bold(),
        "DEBUG" => level.to_uppercase().dimmed(),
        _ => level.to_uppercase().normal(),  // INFO and others
    }
}

fn print_log_entries(entries: &[Value]) {
    for entry in entries {
        let ts  = entry["timestamp"].as_str().unwrap_or("?");
        let fun = entry["function"].as_str().unwrap_or("?");
        let lvl = entry["level"].as_str().unwrap_or("info");
        let msg = entry["message"].as_str().unwrap_or("");

        println!(
            "{}  {}  {}  {}",
            format_timestamp(ts).dimmed(),
            format!("[{}]", fun).cyan(),
            colorize_level(lvl),
            msg
        );
    }
}

// ── API fetch ─────────────────────────────────────────────────────────────

async fn fetch_logs(
    client: &ApiClient,
    function: Option<&str>,
    limit: u64,
    since: Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let mut url = format!("{}/logs?limit={}", client.base_url, limit);
    if let Some(f) = function {
        url.push_str(&format!("&function={}", f));
    }
    if let Some(s) = since {
        url.push_str(&format!("&since={}", urlencoding_simple(s)));
    }

    let res = client.client.get(&url).send().await?;

    if !res.status().is_success() {
        anyhow::bail!("API error: {}", res.status());
    }

    let body: Value = res.json().await?;
    let logs = body["data"]["logs"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(logs)
}

/// Minimal percent-encoding for ':' in ISO timestamps so query params are valid.
fn urlencoding_simple(s: &str) -> String {
    s.replace(':', "%3A")
}

// ── Public entry points ───────────────────────────────────────────────────

/// One-shot log fetch (default).
pub async fn execute(name: Option<String>, limit: u64) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let fn_label = name.as_deref().unwrap_or("all functions");
    println!("\n  {} Logs for {}  (last {})\n", "▸".cyan(), fn_label.bold(), limit);

    let entries = fetch_logs(&client, name.as_deref(), limit, None).await?;

    if entries.is_empty() {
        println!("  {}", "No logs found.".dimmed());
    } else {
        // API returns DESC when no `since`; reverse to show oldest first
        let mut ordered = entries;
        ordered.reverse();
        print_log_entries(&ordered);
    }
    println!();
    Ok(())
}

/// Streaming follow mode — polls every 1.5 s for new log lines.
pub async fn execute_follow(name: Option<String>, limit: u64) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let fn_label = name.as_deref().unwrap_or("all functions");
    println!(
        "\n  {} Following logs for {}  (Ctrl+C to stop)\n",
        "▸".cyan(),
        fn_label.bold()
    );

    // Initial fetch: last `limit` lines
    let initial = fetch_logs(&client, name.as_deref(), limit, None).await?;
    let mut last_timestamp: Option<String> = None;

    if !initial.is_empty() {
        let mut ordered = initial;
        ordered.reverse();   // oldest-first

        // Track the timestamp of the most recent entry
        if let Some(last) = ordered.last() {
            if let Some(ts) = last["timestamp"].as_str() {
                last_timestamp = Some(ts.to_string());
            }
        }
        print_log_entries(&ordered);
    }

    // Poll loop
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(1_500)).await;

        let since = last_timestamp.as_deref();
        let new_entries = match fetch_logs(&client, name.as_deref(), 200, since).await {
            Ok(e) => e,
            Err(_) => {
                // Silently retry on transient errors
                continue;
            }
        };

        if !new_entries.is_empty() {
            // `since` queries return ASC — newest is last
            if let Some(last) = new_entries.last() {
                if let Some(ts) = last["timestamp"].as_str() {
                    last_timestamp = Some(ts.to_string());
                }
            }
            print_log_entries(&new_entries);
        }
    }
}

