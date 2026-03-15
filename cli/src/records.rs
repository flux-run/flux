//! `flux records` — export, count, and prune execution records.
//!
//! Export before the automated retention job runs if you need an archive:
//!
//!   flux records export --before 30d > records-2026-03.jsonl
//!   flux records export --before 30d | aws s3 cp - s3://bucket/flux/2026-03.jsonl
//!   flux records count --before 30d --errors-only
//!   flux records prune --before 30d --dry-run

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

#[derive(Subcommand)]
pub enum RecordsCommands {
    /// Stream execution records to stdout as JSONL (default) or CSV.
    ///
    /// Examples:
    ///   flux records export --before 30d > records.jsonl
    ///   flux records export --before 30d | aws s3 cp - s3://bucket/flux.jsonl
    ///   flux records export --errors-only --format csv
    Export {
        /// Include records older than this age (e.g. 30d, 7d, 24h)
        #[arg(long, value_name = "AGE")]
        before: Option<String>,
        /// Include records newer than this age
        #[arg(long, value_name = "AGE")]
        after: Option<String>,
        /// Filter to a specific function name
        #[arg(long, value_name = "NAME")]
        function: Option<String>,
        /// Only include records where an error occurred
        #[arg(long)]
        errors_only: bool,
        /// Output format (jsonl or csv)
        #[arg(long, default_value = "jsonl", value_name = "FORMAT")]
        format: String,
    },
    /// Count records matching the given filters.
    ///
    /// Use this to preview what the retention job (or `flux records prune`) will delete.
    ///
    /// Examples:
    ///   flux records count --before 30d
    ///   flux records count --before 30d --errors-only
    Count {
        /// Count records older than this age
        #[arg(long, value_name = "AGE")]
        before: Option<String>,
        /// Count records newer than this age
        #[arg(long, value_name = "AGE")]
        after: Option<String>,
        /// Filter to a specific function name
        #[arg(long, value_name = "NAME")]
        function: Option<String>,
        /// Only count records where an error occurred
        #[arg(long)]
        errors_only: bool,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Manually delete old records on demand.
    ///
    /// `--dry-run` behaves identically to `flux records count` with the same filters.
    ///
    /// Examples:
    ///   flux records prune --before 30d --dry-run
    ///   flux records prune --before 30d
    Prune {
        /// Delete records older than this age (e.g. 30d, 7d, 24h)
        #[arg(long, value_name = "AGE")]
        before: Option<String>,
        /// Preview — show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,
        /// Auto-confirm without prompting
        #[arg(long)]
        yes: bool,
    },
}

pub async fn execute(command: RecordsCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        RecordsCommands::Export { before, after, function, errors_only, format } => {
            let mut url = R::records::EXPORT.url(&client.base_url);
            let mut params: Vec<String> = Vec::new();
            if let Some(b) = &before   { params.push(format!("before={}", b)); }
            if let Some(a) = &after    { params.push(format!("after={}", a)); }
            if let Some(f) = &function { params.push(format!("function={}", f)); }
            if errors_only             { params.push("errors_only=true".to_string()); }
            if format != "jsonl"       { params.push(format!("format={}", format)); }
            if !params.is_empty() {
                url = format!("{}?{}", url, params.join("&"));
            }

            let res = client.client.get(&url).send().await?;
            if res.status().is_success() {
                // Stream response body directly to stdout
                let body = res.text().await?;
                print!("{}", body);
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Export failed ({}): {}", status, body);
            }
        }

        RecordsCommands::Count { before, after, function, errors_only, json } => {
            let mut q: Vec<(&str, &str)> = Vec::new();
            if let Some(ref b) = before   { q.push(("before",      b.as_str())); }
            if let Some(ref a) = after    { q.push(("after",       a.as_str())); }
            if let Some(ref f) = function { q.push(("function",    f.as_str())); }
            if errors_only               { q.push(("errors_only", "true")); }
            let resp: Value = client.get_with(&R::records::COUNT, &[], &q).await?;
            let count = resp["count"].as_u64().unwrap_or(0);

            if json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{} records match", count.to_string().bold());
            }
        }

        RecordsCommands::Prune { before, dry_run, yes } => {
            if dry_run {
                // --dry-run: just count with the same filter
                let mut q: Vec<(&str, &str)> = Vec::new();
                if let Some(ref b) = before { q.push(("before", b.as_str())); }
                let resp: Value = client.get_with(&R::records::COUNT, &[], &q).await?;
                let count = resp["count"].as_u64().unwrap_or(0);
                println!(
                    "{} {} records would be deleted (dry run — nothing changed)",
                    "~".dimmed(),
                    count.to_string().bold(),
                );
                return Ok(());
            }

            // Count first, then confirm
            let mut count_q: Vec<(&str, &str)> = Vec::new();
            if let Some(ref b) = before { count_q.push(("before", b.as_str())); }
            let count_resp: Value = client.get_with(&R::records::COUNT, &[], &count_q).await?;
            let count = count_resp["count"].as_u64().unwrap_or(0);

            if count == 0 {
                println!("No records match — nothing to prune.");
                return Ok(());
            }

            if !yes {
                print!(
                    "Permanently delete {} records{}? [y/N]: ",
                    count.to_string().bold(),
                    before.as_deref().map(|b| format!(" older than {}", b)).unwrap_or_default(),
                );
                use std::io::{BufRead, Write};
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                if line.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let mut del_q: Vec<(&str, &str)> = Vec::new();
            if let Some(ref b) = before { del_q.push(("before", b.as_str())); }
            match client.delete_q(&R::records::PRUNE, &del_q).await {
                Ok(resp) => {
                    let resp: Value = resp;
                    let deleted = resp["deleted"].as_u64().unwrap_or(count);
                    println!(
                        "{} Pruned {} records",
                        "✔".green().bold(),
                        deleted.to_string().bold(),
                    );
                }
                Err(e) => anyhow::bail!("Prune failed: {}", e),
            }
        }
    }

    Ok(())
}
