//! `flux serve <file>` — serve any JS/TS file as an HTTP endpoint.
//!
//! [...]

use std::path::PathBuf;
use anyhow::Context;
use colored::Colorize;

pub struct ServeArgs {
    pub file:         PathBuf,
    pub port:         u16,
    pub database_url: Option<String>,
}

/// Execute `flux serve <file>`.
pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    println!();
    println!("{} {}", "◆ flux serve".cyan().bold(), args.file.display());
    println!();

    // Resolve DATABASE_URL
    let database_url = resolve_database_url(args.database_url.as_deref())?;

    // Verify file exists
    if !args.file.exists() {
        anyhow::bail!("File not found: {}", args.file.display());
    }
    let file_abs = args.file.canonicalize()
        .with_context(|| format!("Cannot resolve path: {}", args.file.display()))?;

    println!(
        "  {} database        {}",
        "✔".green().bold(),
        "(DATABASE_URL resolved)".dimmed()
    );
    println!(
        "  {} file            {}",
        "✔".green().bold(),
        file_abs.display().to_string().cyan()
    );

    // Find the server binary
    let binary = find_server_binary()
        .ok_or_else(|| anyhow::anyhow!(
            "Flux server binary not found.\n  Run `cargo build` first, or install Flux."
        ))?;

    println!(
        "  {} server          listening on {}",
        "◆".blue().bold(),
        format!("http://localhost:{}", args.port).cyan()
    );
    println!();
    println!("{}", "  Press Ctrl+C to stop.".dimmed());
    println!();

    let mut cmd = tokio::process::Command::new(&binary);
    cmd.env("PORT", args.port.to_string())
        .env("DATABASE_URL", &database_url)
        .env("FLUX_SERVE_FILE", file_abs.to_string_lossy().as_ref())
        .env("FLUX_LOCAL", "true")
        .env("LOCAL_MODE", "true")
        .env("RUST_LOG", "warn")
        .kill_on_drop(true);

    let mut child = cmd.spawn()
        .with_context(|| format!("Failed to start server binary: {}", binary.display()))?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await.ok();
    let _ = child.kill().await;

    println!("{}", "\n✔  Stopped.".green());
    Ok(())
}

// ── `flux replay` ─────────────────────────────────────────────────────────────

/// Execute `flux replay <execution_id> [--commit] [--from <index>]`.
pub async fn execute_replay(
    execution_id: String,
    commit:       bool,
    from:         Option<u32>,
    database_url: Option<String>,
) -> anyhow::Result<()> {
    let database_url = resolve_database_url(database_url.as_deref())?;
    let pool = connect_db(&database_url).await?;

    let exec_uuid = parse_execution_id(&execution_id)?;

    println!();
    println!("{} {}", "◆ flux replay".cyan().bold(), execution_id.dimmed());
    if commit {
        println!("  {} --commit flag set: DB writes will be applied", "⚠".yellow());
    } else {
        println!("  {} dry run (DB writes suppressed, use --commit to apply)", "ℹ".dimmed());
    }
    if let Some(f) = from {
        println!("  {} resuming from checkpoint {}", "→".cyan(), f);
    }
    println!();

    // Fetch the original execution record
    let orig = fetch_execution_record(&pool, exec_uuid).await?;
    println!("  original:  {} → {}", orig.label.cyan(), orig.status.dimmed());
    println!("  started:   {}", orig.started_at.dimmed());

    // List checkpoints
    let checkpoints = list_checkpoints(&pool, exec_uuid).await?;
    if checkpoints.is_empty() {
        anyhow::bail!("No checkpoints found for execution {}", execution_id);
    }
    println!("  checkpoints: {}", checkpoints.len().to_string().cyan());
    println!();

    for cp in &checkpoints {
        let boundary_icon = if cp.boundary == "http" { "🌐" } else { "🗄" };
        println!(
            "  [{}] {} {:>6}  {}  {}ms",
            cp.call_index.to_string().cyan(),
            boundary_icon,
            cp.status.dimmed(),
            cp.label.cyan(),
            cp.duration_ms,
        );
    }

    println!();
    println!(
        "  {} To re-run this execution with the recorded responses injected,",
        "ℹ".dimmed()
    );
    println!(
        "    set FLUX_REPLAY_EXECUTION_ID={} on your {} process.",
        exec_uuid,
        "flux serve".cyan()
    );
    println!(
        "    The server will inject recorded responses at each boundary crossing."
    );

    pool.close().await;
    Ok(())
}

// ── `flux resume` ─────────────────────────────────────────────────────────────

/// Execute `flux resume <execution_id>`.
pub async fn execute_resume(
    execution_id: String,
    database_url: Option<String>,
) -> anyhow::Result<()> {
    let database_url = resolve_database_url(database_url.as_deref())?;
    let pool = connect_db(&database_url).await?;

    let exec_uuid = parse_execution_id(&execution_id)?;

    println!();
    println!("{} {}", "◆ flux resume".cyan().bold(), execution_id.dimmed());
    println!();

    let orig = fetch_execution_record(&pool, exec_uuid).await?;
    println!("  original:  {} → {}", orig.label.cyan(), orig.status.dimmed());
    println!("  started:   {}", orig.started_at.dimmed());

    let checkpoints = list_checkpoints(&pool, exec_uuid).await?;
    let n = checkpoints.len() as u32;

    println!("  checkpoints: {} recorded", n.to_string().cyan());
    println!();

    if n == 0 {
        anyhow::bail!("No checkpoints found — cannot resume an execution with no recorded calls.");
    }

    println!(
        "  {} Resume will fast-forward through {} checkpoint{},",
        "ℹ".dimmed(),
        n,
        if n == 1 { "" } else { "s" }
    );
    println!(
        "    then continue live from checkpoint index {}.",
        n.to_string().cyan()
    );
    println!();
    println!(
        "  {} To resume, set FLUX_RESUME_EXECUTION_ID={} FLUX_RESUME_FROM={}",
        "→".cyan(),
        exec_uuid,
        n
    );
    println!(
        "    on your {} process and re-send the original request.",
        "flux serve".cyan()
    );

    pool.close().await;
    Ok(())
}

// ── `flux checkpoint` ─────────────────────────────────────────────────────────

/// Execute `flux checkpoint <execution_id>`.
pub async fn execute_checkpoint(
    execution_id: String,
    database_url: Option<String>,
) -> anyhow::Result<()> {
    let database_url = resolve_database_url(database_url.as_deref())?;
    let pool = connect_db(&database_url).await?;

    let exec_uuid = parse_execution_id(&execution_id)?;

    let checkpoints = list_checkpoints(&pool, exec_uuid).await?;
    if checkpoints.is_empty() {
        println!("No checkpoints recorded for execution {}.", execution_id.cyan());
        pool.close().await;
        return Ok(());
    }

    println!();
    println!(
        "  {:<6}  {:<6}  {:<50}  {:<8}  {}",
        "INDEX".dimmed(), "TYPE".dimmed(), "URL / QUERY".dimmed(),
        "DURATION".dimmed(), "STATUS".dimmed()
    );
    println!("  {}", "─".repeat(90).dimmed());

    for cp in &checkpoints {
        println!(
            "  {:<6}  {:<6}  {:<50}  {:>6}ms  {}",
            cp.call_index.to_string().cyan(),
            cp.boundary.dimmed(),
            truncate(&cp.label, 50).cyan(),
            cp.duration_ms,
            if cp.status.is_empty() { "—".to_string() } else { cp.status.clone() },
        );
    }
    println!();

    pool.close().await;
    Ok(())
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn resolve_database_url(flag: Option<&str>) -> anyhow::Result<String> {
    if let Some(url) = flag {
        return Ok(url.to_string());
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.is_empty() {
            return Ok(url);
        }
    }
    // Try .env file in cwd
    if let Ok(iter) = dotenvy::from_path_iter(".env") {
        for item in iter.flatten() {
            if item.0 == "DATABASE_URL" && !item.1.is_empty() {
                return Ok(item.1);
            }
        }
    }
    anyhow::bail!(
        "\n{}\n\nFlux requires a Postgres database. Set DATABASE_URL and retry.\n\nQuick start:\n  {}\n  {}\n",
        "ERROR: DATABASE_URL is not set.".red().bold(),
        "docker run -p 5432:5432 -e POSTGRES_PASSWORD=flux postgres:16".cyan(),
        "export DATABASE_URL=postgres://postgres:flux@localhost/flux".cyan(),
    )
}

async fn connect_db(database_url: &str) -> anyhow::Result<sqlx::PgPool> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await
        .with_context(|| format!("Failed to connect to database: {}", database_url))
}

fn parse_execution_id(s: &str) -> anyhow::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(s)
        .with_context(|| format!("'{}' is not a valid execution ID (UUID expected)", s))
}

fn find_server_binary() -> Option<std::path::PathBuf> {
    // 1. Alongside the CLI binary
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent().map(|p| p.join("server"));
        if let Some(s) = sibling {
            if s.exists() {
                return Some(s);
            }
        }
    }
    // 2. Cargo workspace target/debug
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("target").join("debug").join("server"));
    if let Some(p) = workspace {
        if p.exists() {
            return Some(p);
        }
    }
    // 3. PATH
    which::which("server").ok()
}

#[derive(Debug)]
struct ExecutionRecord {
    label:      String,
    status:     String,
    started_at: String,
}

async fn fetch_execution_record(pool: &sqlx::PgPool, id: uuid::Uuid) -> anyhow::Result<ExecutionRecord> {
    #[derive(sqlx::FromRow)]
    struct Row {
        label:      String,
        status:     String,
        started_at: chrono::DateTime<chrono::Utc>,
    }

    let row: Row = sqlx::query_as::<_, Row>(
        "SELECT label, status, started_at FROM flux.execution_records WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("Failed to fetch execution record {}", id))?
    .ok_or_else(|| anyhow::anyhow!("Execution '{}' not found", id))?;

    Ok(ExecutionRecord {
        label:      row.label,
        status:     row.status,
        started_at: row.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    })
}

#[derive(Debug)]
struct CheckpointSummary {
    call_index:  u32,
    boundary:    String,
    label:       String,
    duration_ms: u32,
    status:      String,
}

async fn list_checkpoints(pool: &sqlx::PgPool, execution_id: uuid::Uuid) -> anyhow::Result<Vec<CheckpointSummary>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        call_index:  i32,
        boundary:    String,
        request:     Vec<u8>,
        response:    Vec<u8>,
        duration_ms: i32,
    }

    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT call_index, boundary, request, response, duration_ms \
         FROM flux.checkpoints \
         WHERE execution_id = $1 \
         ORDER BY call_index ASC"
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .with_context(|| format!("Failed to list checkpoints for {}", execution_id))?;

    Ok(rows.into_iter().map(|r| {
        let label = serde_json::from_slice::<serde_json::Value>(&r.request)
            .ok()
            .map(|v| {
                if r.boundary == "http" {
                    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("GET");
                    let url    = v.get("url").and_then(|u| u.as_str()).unwrap_or("?");
                    format!("{} {}", method, url)
                } else {
                    v.get("query").and_then(|q| q.as_str()).unwrap_or("?").to_string()
                }
            })
            .unwrap_or_default();

        let status = serde_json::from_slice::<serde_json::Value>(&r.response)
            .ok()
            .map(|v| {
                if r.boundary == "http" {
                    v.get("status").and_then(|s| s.as_u64()).map(|s| s.to_string()).unwrap_or_default()
                } else {
                    "ok".to_string()
                }
            })
            .unwrap_or_default();

        CheckpointSummary {
            call_index:  r.call_index as u32,
            boundary:    r.boundary,
            label,
            duration_ms: r.duration_ms as u32,
            status,
        }
    }).collect())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
