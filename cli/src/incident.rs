//! `flux incident` — incident investigation and replay commands.
//!
//! These commands use the deterministic replay architecture to replay
//! past requests against the current data state without triggering
//! side effects (hooks, events, workflows).
//!
//! ```text
//! flux incident replay 2026-03-11T15:00:00Z..2026-03-11T15:05:00Z
//! flux incident replay --request-id 550e8400
//! ```

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

// ── Subcommand definitions ───────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum IncidentCommands {
    /// Replay a time window or single request in replay mode (x-flux-replay: true).
    ///
    /// Replay re-applies state mutations WITHOUT triggering hooks, events,
    /// workflows, or any other side effects.  Safe to run against production data.
    ///
    /// Examples:
    ///   flux incident replay 2026-03-11T15:00:00Z..2026-03-11T15:05:00Z
    ///   flux incident replay --request-id 550e8400
    ///   flux incident replay --from 2026-03-11T15:00:00Z --to 2026-03-11T15:05:00Z
    Replay {
        /// Time window as "from..to" (RFC-3339 timestamps), e.g. "2026-03-11T15:00:00Z..2026-03-11T15:05:00Z"
        window: Option<String>,
        /// Replay a single request by ID instead of a time window
        #[arg(long, value_name = "REQUEST_ID")]
        request_id: Option<String>,
        /// Start of time window (alternative to positional window)
        #[arg(long, value_name = "RFC3339")]
        from: Option<String>,
        /// End of time window (alternative to positional window)
        #[arg(long, value_name = "RFC3339")]
        to: Option<String>,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Confirm replay without prompting
        #[arg(long)]
        yes: bool,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn execute(command: IncidentCommands) -> anyhow::Result<()> {
    match command {
        IncidentCommands::Replay {
            window,
            request_id,
            from,
            to,
            database,
            yes,
            json,
        } => execute_replay(window, request_id, from, to, database, yes, json).await,
    }
}

// ── replay ───────────────────────────────────────────────────────────────────

async fn execute_replay(
    window: Option<String>,
    request_id_filter: Option<String>,
    from: Option<String>,
    to: Option<String>,
    database: String,
    yes: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // ── Mode 1: replay a single request by ID ───────────────────────────────
    if let Some(ref rid) = request_id_filter {
        return replay_single_request(&client, rid, &database, yes, json_output).await;
    }

    // ── Mode 2: replay a time window ─────────────────────────────────────────
    let (from_ts, to_ts) = resolve_window(window, from, to)?;

    // Fetch all mutations in the window via /db/replay/:database
    let url = format!(
        "{}/db/replay/{}?from={}&to={}&limit=2000",
        client.base_url,
        database,
        urlencoding::encode(&from_ts),
        urlencoding::encode(&to_ts),
    );

    let res = client.client.get(&url).send().await?;
    if !res.status().is_success() {
        anyhow::bail!("API error {}: {}", res.status(), res.text().await.unwrap_or_default());
    }
    let body: Value = res.json().await?;

    let empty_vec = vec![];
    let mutations: &Vec<Value> = body
        .get("mutations")
        .and_then(|m| m.as_array())
        .unwrap_or(&empty_vec);

    if mutations.is_empty() {
        println!(
            "{}",
            format!("No mutations found in window {}..{}", from_ts, to_ts).dimmed()
        );
        return Ok(());
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(mutations)?);
        return Ok(());
    }

    println!();
    println!(
        "{} {}",
        "Incident Replay".bold(),
        format!("({} mutations in window)", mutations.len()).dimmed(),
    );
    println!("{}", "─".repeat(60).dimmed());
    println!("  window:    {} → {}", from_ts, to_ts);
    println!("  database:  {}", database.cyan());
    println!(
        "  mode:      {} (hooks / events / workflows {})",
        "x-flux-replay: true".yellow(),
        "SUPPRESSED".red().bold(),
    );
    println!();

    if !yes {
        println!(
            "{}",
            "This will re-apply mutations to data without triggering side effects.".yellow()
        );
        print!("  Continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
        println!();
    }

    // Group mutations by request_id to replay in request-level batches
    let mut request_groups: Vec<(String, Vec<&Value>)> = Vec::new();
    for m in mutations {
        let rid = m["request_id"].as_str().unwrap_or("-").to_string();
        if let Some(last) = request_groups.last_mut() {
            if last.0 == rid {
                last.1.push(m);
                continue;
            }
        }
        request_groups.push((rid, vec![m]));
    }

    let total = request_groups.len();
    let mut replayed = 0usize;
    let mut failed   = 0usize;

    for (idx, (rid, group)) in request_groups.iter().enumerate() {
        print!(
            "  [{}/{}]  request {}  ({} mutation{}) … ",
            idx + 1,
            total,
            &rid[..rid.len().min(12)].dimmed(),
            group.len(),
            if group.len() == 1 { "" } else { "s" },
        );
        use std::io::Write;
        std::io::stdout().flush()?;

        // Replay each mutation as a db/query call with x-flux-replay: true
        let mut ok = true;
        for m in group {
            let table     = m["table_name"].as_str().unwrap_or_default();
            let operation = m["operation"].as_str().unwrap_or_default();
            let data      = m["after_state"].clone();
            let pk        = m["record_pk"].clone();

            // Build query payload based on operation
            let payload = match operation {
                "insert" => serde_json::json!({
                    "database":  database,
                    "table":     table,
                    "operation": "insert",
                    "data":      data,
                }),
                "update" => serde_json::json!({
                    "database":  database,
                    "table":     table,
                    "operation": "update",
                    "data":      data,
                    "filters":   pk,
                }),
                "delete" => serde_json::json!({
                    "database":  database,
                    "table":     table,
                    "operation": "delete",
                    "filters":   pk,
                }),
                _ => continue,
            };

            let qres = client.client
                .post(&format!("{}/db/query", client.base_url))
                .header("x-flux-replay", "true")
                .header("x-request-id", &format!("replay:{}", rid))
                .json(&payload)
                .send()
                .await;

            match qres {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => {
                    let body_txt = r.text().await.unwrap_or_default();
                    eprintln!("\n    {} {}.{}: {}", "✗".red(), table, operation, body_txt.dimmed());
                    ok = false;
                }
                Err(e) => {
                    eprintln!("\n    {} network error: {}", "✗".red(), e);
                    ok = false;
                }
            }
        }

        if ok {
            println!("{}", "✔".green());
            replayed += 1;
        } else {
            failed += 1;
        }
    }

    println!();
    println!(
        "{}  replayed {} request{}, {} failed",
        if failed == 0 { "✔".green().bold().to_string() } else { "✗".red().bold().to_string() },
        replayed,
        if replayed == 1 { "" } else { "s" },
        failed,
    );
    println!();
    Ok(())
}

// ── replay single request ────────────────────────────────────────────────────

async fn replay_single_request(
    client: &ApiClient,
    request_id: &str,
    database: &str,
    yes: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    // Fetch mutations for this request
    let url = format!(
        "{}/db/mutations?request_id={}&limit=200",
        client.base_url, request_id
    );
    let res = client.client.get(&url).send().await?;
    if !res.status().is_success() {
        anyhow::bail!("API error {}: {}", res.status(), res.text().await.unwrap_or_default());
    }
    let body: Value = res.json().await?;

    let empty_vec = vec![];
    let mutations: &Vec<Value> = body
        .get("mutations")
        .and_then(|m| m.as_array())
        .unwrap_or(&empty_vec);

    if mutations.is_empty() {
        println!(
            "{}",
            format!("No mutations found for request_id: {}", request_id).dimmed()
        );
        return Ok(());
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    println!();
    println!(
        "{} {} ({} mutation{})",
        "Replaying request:".bold(),
        &request_id[..request_id.len().min(24)].cyan(),
        mutations.len(),
        if mutations.len() == 1 { "" } else { "s" },
    );
    println!(
        "  mode: {} (side effects {})",
        "x-flux-replay: true".yellow(),
        "SUPPRESSED".red().bold(),
    );
    println!();

    if !yes {
        print!("  Continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
        println!();
    }

    let mut ok_count = 0usize;
    let mut err_count = 0usize;

    for (i, m) in mutations.iter().enumerate() {
        let table     = m["table_name"].as_str().unwrap_or_default();
        let operation = m["operation"].as_str().unwrap_or_default();
        let data      = m["after_state"].clone();
        let pk        = m["record_pk"].clone();

        let op_label = match operation {
            "insert" => operation.green().bold().to_string(),
            "update" => operation.yellow().bold().to_string(),
            "delete" => operation.red().bold().to_string(),
            _        => operation.normal().to_string(),
        };
        print!(
            "  [{}/{}]  {}.{}  {} … ",
            i + 1, mutations.len(), table.cyan(), operation, op_label,
        );
        use std::io::Write;
        std::io::stdout().flush()?;

        let payload = match operation {
            "insert" => serde_json::json!({
                "database": database, "table": table,
                "operation": "insert", "data": data,
            }),
            "update" => serde_json::json!({
                "database": database, "table": table,
                "operation": "update", "data": data, "filters": pk,
            }),
            "delete" => serde_json::json!({
                "database": database, "table": table,
                "operation": "delete", "filters": pk,
            }),
            _ => { println!("{}", "skipped".dimmed()); continue; }
        };

        let qres = client.client
            .post(&format!("{}/db/query", client.base_url))
            .header("x-flux-replay", "true")
            .header("x-request-id", &format!("replay:{}", request_id))
            .json(&payload)
            .send()
            .await;

        match qres {
            Ok(r) if r.status().is_success() => {
                println!("{}", "✔".green());
                ok_count += 1;
            }
            Ok(r) => {
                let t = r.text().await.unwrap_or_default();
                println!("{} {}", "✗".red(), t.dimmed());
                err_count += 1;
            }
            Err(e) => {
                println!("{} {}", "✗".red(), e);
                err_count += 1;
            }
        }
    }

    println!();
    println!(
        "{}  {} applied, {} failed",
        if err_count == 0 { "✔".green().bold().to_string() } else { "✗".red().bold().to_string() },
        ok_count,
        err_count,
    );
    println!();
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parse the time window from either a positional "from..to" arg or --from/--to flags.
fn resolve_window(
    window: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> anyhow::Result<(String, String)> {
    if let Some(w) = window {
        let parts: Vec<&str> = w.splitn(2, "..").collect();
        if parts.len() != 2 {
            anyhow::bail!("Window must be in format 'from..to', e.g. '2026-03-11T15:00:00Z..2026-03-11T15:05:00Z'");
        }
        return Ok((parts[0].to_string(), parts[1].to_string()));
    }
    match (from, to) {
        (Some(f), Some(t)) => Ok((f, t)),
        _ => anyhow::bail!(
            "Provide either a positional window ('from..to') or both --from and --to flags"
        ),
    }
}
