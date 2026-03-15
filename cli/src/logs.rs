use api_contract::routes as R;
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
        "ERROR" | "ERR"      => level.to_uppercase().red().bold(),
        "WARN"  | "WARNING"  => level.to_uppercase().yellow().bold(),
        "DEBUG"              => level.to_uppercase().dimmed(),
        _                    => level.to_uppercase().normal(),
    }
}

fn colorize_source(source: &str) -> colored::ColoredString {
    match source {
        "db"       => source.magenta(),
        "workflow" => source.yellow(),
        "queue"    => source.blue(),
        "event"    => source.cyan(),
        "system"   => source.dimmed(),
        _          => source.green(),  // function (default)
    }
}

fn print_log_entries(entries: &[Value]) {
    for entry in entries {
        let ts       = entry["timestamp"].as_str().unwrap_or("?");
        // Prefer new unified fields; fall back to legacy "function"
        let source   = entry["source"].as_str().unwrap_or("function");
        let resource = entry["resource"].as_str()
            .or_else(|| entry["function"].as_str())
            .unwrap_or("?");
        let lvl      = entry["level"].as_str().unwrap_or("info");
        let msg      = entry["message"].as_str().unwrap_or("");

        println!(
            "{}  {}  {}  {}",
            format_timestamp(ts).dimmed(),
            format!("[{}/{}]", colorize_source(source), resource.bold()),
            colorize_level(lvl),
            msg
        );
    }
}

// ── API fetch ─────────────────────────────────────────────────────────────

/// Low-level fetch. Supports both the new (source + resource) and legacy
/// (function=) query params.
async fn fetch_logs(
    client:   &ApiClient,
    source:   Option<&str>,
    resource: Option<&str>,
    limit:    u64,
    since:    Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let limit_s = limit.to_string();
    let mut q: Vec<(&str, &str)> = vec![("limit", &limit_s)];
    match (source, resource) {
        (Some(s), Some(r)) => { q.push(("source", s)); q.push(("resource", r)); }
        (None, Some(r))    => { q.push(("source", "function")); q.push(("resource", r)); }
        (Some(s), None)    => { q.push(("source", s)); }
        (None, None)       => {}
    }
    if let Some(s) = since { q.push(("since", s)); }

    let body: Value = client.get_with(&R::logs::LIST, &[], &q).await?;
    Ok(body["data"]["logs"].as_array().cloned().unwrap_or_default())
}

/// Minimal percent-encoding for ':' in ISO timestamps so query params are valid.
fn urlencoding_simple(s: &str) -> String {
    s.replace(':', "%3A")
}

// ── Label helper ──────────────────────────────────────────────────────────

fn display_label(source: Option<&str>, resource: Option<&str>) -> String {
    match (source, resource) {
        (Some(s), Some(r)) => format!("{}/{}", s, r),
        (None,    Some(r)) => format!("function/{}", r),
        (Some(s), None   ) => format!("{} (all)", s),
        (None,    None   ) => "all".to_string(),
    }
}

// ── Public entry points ───────────────────────────────────────────────────

/// One-shot log fetch.
///   flux logs                         → all logs in project
///   flux logs function echo           → function/echo logs
///   flux logs db users                → db/users logs
///   flux logs echo                    → backward compat → function/echo
pub async fn execute(
    source:   Option<String>,
    resource: Option<String>,
    limit:    u64,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let label  = display_label(source.as_deref(), resource.as_deref());

    println!("\n  {} Logs for {}  (last {})\n", "▸".cyan(), label.bold(), limit);

    let entries = fetch_logs(&client, source.as_deref(), resource.as_deref(), limit, None).await?;

    if entries.is_empty() {
        println!("  {}", "No logs found.".dimmed());
    } else {
        let mut ordered = entries;
        ordered.reverse();  // API returns DESC when no `since`; display oldest-first
        print_log_entries(&ordered);
    }
    println!();
    Ok(())
}

/// Streaming follow mode — polls every 1.5 s for new log lines.
pub async fn execute_follow(
    source:   Option<String>,
    resource: Option<String>,
    limit:    u64,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let label  = display_label(source.as_deref(), resource.as_deref());

    println!("\n  {} Following logs for {}  (Ctrl+C to stop)\n", "▸".cyan(), label.bold());

    let initial = fetch_logs(&client, source.as_deref(), resource.as_deref(), limit, None).await?;
    let mut last_timestamp: Option<String> = None;

    if !initial.is_empty() {
        let mut ordered = initial;
        ordered.reverse();
        if let Some(last) = ordered.last() {
            if let Some(ts) = last["timestamp"].as_str() {
                last_timestamp = Some(ts.to_string());
            }
        }
        print_log_entries(&ordered);
    }

    const MIN_POLL_MS: u64 = 1_500;
    const MAX_POLL_MS: u64 = 10_000;
    let mut poll_ms = MIN_POLL_MS;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(poll_ms)).await;

        let since = last_timestamp.as_deref();
        let new_entries = match fetch_logs(&client, source.as_deref(), resource.as_deref(), 200, since).await {
            Ok(e) => e,
            Err(_) => {
                poll_ms = (poll_ms * 2).min(MAX_POLL_MS);
                continue;
            }
        };

        if !new_entries.is_empty() {
            if let Some(last) = new_entries.last() {
                if let Some(ts) = last["timestamp"].as_str() {
                    last_timestamp = Some(ts.to_string());
                }
            }
            print_log_entries(&new_entries);
            poll_ms = MIN_POLL_MS;
        } else {
            poll_ms = (poll_ms * 2).min(MAX_POLL_MS);
        }
    }
}

