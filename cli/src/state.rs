//! `flux state` — row-level state audit commands.
//!
//! These commands expose the `state_mutations` audit log at the row level,
//! answering "what happened to this specific row" and "who last touched each row".
//!
//! ```text
//! flux state history users --id 42         # full version history for row id=42
//! flux state blame   users                 # last writer per row in table
//! ```

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use api_contract::routes as R;
use crate::client::ApiClient;

// ── Subcommand definitions ───────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum StateCommands {
    /// Show every mutation recorded for a single row (full version history).
    ///
    /// Example:
    ///   flux state history users --id 42
    ///   flux state history orders --id ord_abc --database analytics
    History {
        /// Table name
        table: String,
        /// Row primary key (scalar id, e.g. 42 or "abc")
        #[arg(long)]
        id: Option<String>,
        /// Composite/custom pk as JSON (e.g. '{"order_id":99}')
        #[arg(long)]
        pk: Option<String>,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Max versions to display (default 50)
        #[arg(long, default_value = "50")]
        limit: u32,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Show who last modified each row in a table (blame view).
    ///
    /// Example:
    ///   flux state blame users
    ///   flux state blame orders --database analytics --limit 20
    Blame {
        /// Table name
        table: String,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Max rows (default 100)
        #[arg(long, default_value = "100")]
        limit: u32,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn execute(command: StateCommands) -> anyhow::Result<()> {
    match command {
        StateCommands::History { table, id, pk, database, limit, json } => {
            execute_history(table, id, pk, database, limit, json).await
        }
        StateCommands::Blame { table, database, limit, json } => {
            execute_blame(table, database, limit, json).await
        }
    }
}

// ── history ──────────────────────────────────────────────────────────────────

async fn execute_history(
    table: String,
    id: Option<String>,
    pk: Option<String>,
    database: String,
    limit: u32,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let limit_s = limit.to_string();
    let mut query = vec![("limit", limit_s.as_str())];
    if let Some(ref v) = id { query.push(("id", v.as_str())); }
    if let Some(ref v) = pk { query.push(("pk", v.as_str())); }
    let body: Value = client
        .get_with(&R::db::HISTORY, &[("database", &database), ("table", &table)], &query)
        .await?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    let empty_vec = vec![];
    let rows: &Vec<Value> = body
        .get("history")
        .and_then(|h| h.as_array())
        .unwrap_or(&empty_vec);

    if rows.is_empty() {
        println!("{}", "No mutation history found for this row.".dimmed());
        return Ok(());
    }

    let id_label = id.as_deref().or(pk.as_deref()).unwrap_or("(all)");
    println!();
    println!(
        "{} {}.{} id={}  ({} version{})",
        "State history:".bold(),
        database.cyan(),
        table.cyan(),
        id_label,
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
    );
    println!("{}", "─".repeat(72).dimmed());
    println!(
        "  {:<5}  {:<8}  {:<12}  {:<20}  {}",
        "ver".bold(),
        "op".bold(),
        "by".bold(),
        "at".bold(),
        "request_id".bold(),
    );
    println!("{}", "─".repeat(72).dimmed());

    for row in rows {
        let version    = row["version"].as_i64().unwrap_or(0);
        let op         = row["operation"].as_str().unwrap_or("?");
        let actor      = row["actor_id"].as_str().unwrap_or("-");
        let request_id = row["request_id"].as_str().unwrap_or("-");
        let created_at = row["created_at"].as_str()
            .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
            .unwrap_or_default();

        let op_colored = match op {
            "insert" => op.green().bold().to_string(),
            "update" => op.yellow().bold().to_string(),
            "delete" => op.red().bold().to_string(),
            _        => op.normal().to_string(),
        };

        // Truncate request_id to 12 chars for display
        let rid_short = &request_id[..request_id.len().min(12)];

        println!(
            "  {:<5}  {:<8}  {:<12}  {:<20}  {}",
            version,
            op_colored,
            &actor[..actor.len().min(12)],
            created_at,
            rid_short.dimmed(),
        );

        // Show a compact diff if it's an update
        if op == "update" {
            if let (Some(before), Some(after)) =
                (row["before_state"].as_object(), row["after_state"].as_object())
            {
                let changed: Vec<String> = after
                    .iter()
                    .filter(|(k, v)| before.get(k.as_str()).map_or(true, |b| b != *v))
                    .take(4)
                    .map(|(k, v)| {
                        let old = before.get(k.as_str())
                            .map(|b| b.to_string())
                            .unwrap_or_else(|| "null".to_string());
                        let new = v.to_string();
                        format!(
                            "    {} {} → {}",
                            k.dimmed(),
                            old.red(),
                            new.green(),
                        )
                    })
                    .collect();
                if !changed.is_empty() {
                    for line in &changed {
                        println!("{}", line);
                    }
                }
            }
        }
    }
    println!();
    Ok(())
}

// ── blame ────────────────────────────────────────────────────────────────────

async fn execute_blame(
    table: String,
    database: String,
    limit: u32,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let limit_s = limit.to_string();
    let body: Value = client
        .get_with(&R::db::BLAME, &[("database", &database), ("table", &table)], &[("limit", limit_s.as_str())])
        .await?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    let empty_vec = vec![];
    let rows: &Vec<Value> = body
        .get("blame")
        .and_then(|b| b.as_array())
        .unwrap_or(&empty_vec);

    if rows.is_empty() {
        println!("{}", "No blame data found for this table.".dimmed());
        return Ok(());
    }

    println!();
    println!(
        "{} {}.{}",
        "State blame:".bold(),
        database.cyan(),
        table.cyan(),
    );
    println!("{}", "─".repeat(72).dimmed());
    println!(
        "  {:<30}  {:<12}  {:<5}  {:<20}  {}",
        "record_pk".bold(),
        "last_by".bold(),
        "ver".bold(),
        "last_at".bold(),
        "request_id".bold(),
    );
    println!("{}", "─".repeat(72).dimmed());

    for row in rows {
        let pk         = row["record_pk"].to_string();
        let actor      = row["actor_id"].as_str().unwrap_or("-");
        let version    = row["version"].as_i64().unwrap_or(0);
        let request_id = row["request_id"].as_str().unwrap_or("-");
        let created_at = row["created_at"].as_str()
            .map(|s| s.get(..19).unwrap_or(s).replace('T', " "))
            .unwrap_or_default();

        let rid_short = &request_id[..request_id.len().min(12)];

        println!(
            "  {:<30}  {:<12}  {:<5}  {:<20}  {}",
            &pk[..pk.len().min(30)],
            &actor[..actor.len().min(12)],
            version,
            created_at,
            rid_short.dimmed(),
        );
    }
    println!();
    Ok(())
}
