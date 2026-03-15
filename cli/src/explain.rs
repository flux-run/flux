//! `flux explain <request.json>` — dry-run a query and show the full compiler output.
//!
//! Runs the same pipeline as a real query — auth → guard → policy → compile —
//! but stops before execution.  Nothing is written to or read from the database.
//!
//! ```text
//! flux explain create_user.json
//!
//! ─── Query Plan ────────────────────────────────────────────────────────────
//!   Table:     users
//!   Operation: select
//!   Schema:    t_acme_auth_main
//!   Database:  default
//!
//! ─── Policies Applied ──────────────────────────────────────────────────────
//!   Role:      authenticated
//!   Columns:   (all allowed)
//!   Row filter: tenant_id = $1   →  ["5b5f77d1-ce22-4439-8d81-b35c9ecb292e"]
//!
//! ─── Compiled SQL ──────────────────────────────────────────────────────────
//!   SELECT id, name, email FROM t_acme_auth_main.users
//!   WHERE active = true AND tenant_id = $1 LIMIT 50
//!
//! ─── QueryGuard Score ──────────────────────────────────────────────────────
//!   Filters:         2
//!   Selector depth:  0
//!   Complexity:      4  (max: 500)  ✓ within limits
//! ```

use std::path::PathBuf;
use anyhow::Context;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;
use api_contract::routes as R;

fn section(title: &str) {
    let bar = "─".repeat(54);
    println!("\n{} {}", format!("─── {}", title).bold(), bar.dimmed());
}

pub async fn execute(file: Option<PathBuf>, json_output: bool) -> anyhow::Result<()> {
    // ── 1. Load request body ─────────────────────────────────────────────────
    let body_str = match file {
        Some(ref path) if path.to_str() == Some("-") => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).context("failed to read stdin")?;
            buf
        }
        Some(ref path) => {
            std::fs::read_to_string(path)
                .with_context(|| format!("Cannot read file: {}", path.display()))?
        }
        None => {
            anyhow::bail!(
                "Provide a query JSON file:\n  flux explain request.json\n\nOr pipe JSON:\n  cat request.json | flux explain -"
            );
        }
    };

    // Validate it's JSON before sending.
    let _parsed: Value = serde_json::from_str(&body_str)
        .context("request file is not valid JSON")?;

    // ── 2. Call /db/explain ──────────────────────────────────────────────────
    let client = ApiClient::new().await?;
    let url = R::db::EXPLAIN.url(&client.base_url);

    let resp = client
        .client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body_str)
        .send()
        .await
        .context("request to /db/explain failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("could not parse /db/explain response")?;

    if !status.is_success() {
        let msg = body
            .get("message")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("explain failed ({}): {}", status.as_u16(), msg);
    }

    // ── 3. Output ────────────────────────────────────────────────────────────
    if json_output {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    // ─── Query Plan ──────────────────────────────────────────────────────────
    section("Query Plan");
    let qp = &body["query_plan"];
    println!("  Table:     {}", qp["table"].as_str().unwrap_or("-").bold());
    println!("  Operation: {}", qp["operation"].as_str().unwrap_or("-").cyan());
    println!("  Schema:    {}", qp["schema"].as_str().unwrap_or("-").dimmed());
    println!("  Database:  {}", qp["database"].as_str().unwrap_or("default").dimmed());

    // ─── Policies Applied ─────────────────────────────────────────────────────
    section("Policies Applied");
    let pol = &body["policies_applied"];
    println!("  Role:      {}", pol["role"].as_str().unwrap_or("-").yellow());

    let allowed_cols = pol["allowed_columns"]
        .as_array()
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        });
    match allowed_cols {
        Some(cols) => println!("  Columns:   {}", cols),
        None       => println!("  Columns:   {}", "(all allowed)".green()),
    }

    match pol["row_condition"].as_str() {
        None => println!("  Row filter: {}", "none".dimmed()),
        Some(cond) => {
            let params_display = pol["row_params"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .map(|p| format!("  →  [{}]", p))
                .unwrap_or_default();
            println!("  Row filter: {}{}", cond.italic(), params_display.dimmed());
        }
    }

    // ─── Compiled SQL ─────────────────────────────────────────────────────────
    section("Compiled SQL");
    let sql = body["compiled_sql"].as_str().unwrap_or("-");
    // Indent multi-line SQL for readability.
    for line in sql.lines() {
        println!("  {}", line.bright_white());
    }

    // ─── QueryGuard Score ─────────────────────────────────────────────────────
    section("QueryGuard Score");
    let g = &body["guard"];
    let score   = g["complexity_score"].as_u64().unwrap_or(0);
    let max     = g["max_complexity"].as_u64().unwrap_or(500);
    let over    = g["over_limit"].as_bool().unwrap_or(false);
    let filters = g["filters"].as_u64().unwrap_or(0);
    let depth   = g["selector_depth"].as_u64().unwrap_or(0);

    println!("  Filters:         {}", filters);
    println!("  Selector depth:  {}", depth);

    let score_str = format!("{}  (max: {})", score, max);
    if over {
        println!(
            "  Complexity:      {}  {} — would be rejected by QueryGuard",
            score_str.red(),
            "✗ over limit".red().bold()
        );
    } else {
        println!("  Complexity:      {}  {}", score_str, "✓ within limits".green());
    }
    println!();

    Ok(())
}
