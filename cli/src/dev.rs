//! `flux dev` — zero-config local development server.
//!
//! Starts the full Flux stack with NO external dependencies:
//!
//! ```text
//! $ flux dev
//!
//! ◆ Starting Flux dev server…
//!
//!   ↓ PostgreSQL 16  (first run: downloading ~50MB, cached forever after)
//!   ✔ postgres       localhost:5433
//!   ✔ migrations     52 applied
//!   ✔ flux server    localhost:4000
//!
//!   Flux   http://localhost:4000
//!   API    http://localhost:4000/flux/api
//!   Dash   http://localhost:4000/flux
//!
//!   flux invoke <fn>    — call a function
//!   flux trace <id>     — inspect a request
//!   flux why <id>       — root-cause an error
//!   flux generate       — regenerate ctx types
//!
//!   Press Ctrl+C to stop.
//! ```
//!
//! ## What happens
//!
//! 1. Embedded PostgreSQL is downloaded once to `~/.flux/cache/postgres/`
//!    and reused on every subsequent `flux dev` run.
//! 2. A per-project data directory is created at `.flux/dev/pgdata/`.
//! 3. All migrations from `schemas/api/` and `schemas/data-engine/` are
//!    applied in filename order (both sets are idempotent — safe to re-run).
//! 4. The Flux server binary is found via the same resolution as `flux server`:
//!    alongside the flux binary, then workspace target/debug, then PATH.
//! 5. Ctrl+C stops the server process, then stops PostgreSQL.
//!
//! ## SOLID
//!
//! - SRP: `start_postgres`, `run_migrations`, `start_server`, `print_banner`
//!   each do exactly one thing.
//! - DIP: PostgreSQL is behind the `postgresql_embedded::PostgreSQL` type;
//!   the server is started via `tokio::process::Command`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use colored::Colorize;
use tokio::process::Command;
use tokio::signal;

use crate::config::DEFAULT_SERVER_PORT;

// Starting port when searching for a free one. 5432 is intentionally skipped
// so a system Postgres is never accidentally used or conflicted with.
const DEV_DB_PORT_START: u16 = 5433;
const DEV_DB_NAME: &str = "fluxbase_dev";
const DEV_DB_USER: &str = "flux";
const DEV_DB_PASS: &str = "fluxdev";

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute() -> anyhow::Result<()> {
    println!();
    println!("{}", "◆ Starting Flux dev server…".cyan().bold());
    println!();

    // 1. Locate project root for relative paths (schemas/, .flux/)
    let project_root = find_project_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // 2. Resolve DATABASE_URL — check env var, then .env file, then embed Postgres.
    //
    //    Priority:
    //      a) DATABASE_URL shell env var  (already set)
    //      b) DATABASE_URL in .env file   (at project root)
    //      c) Embedded Postgres           (zero-config default)
    let (database_url, _pg) = resolve_database(&project_root).await?;

    // 3. Run all migrations
    let migration_count = run_migrations(&database_url, &project_root).await?;
    println!(
        "  {} migrations     {} applied",
        "✔".green().bold(),
        migration_count.to_string().cyan(),
    );

    // 4. Start Flux server
    let mut server = start_server(DEFAULT_SERVER_PORT, &database_url).await?;

    // 5. Print banner
    print_banner(DEFAULT_SERVER_PORT);

    // 6. Wait for Ctrl+C or server exit
    tokio::select! {
        result = server.wait() => {
            let status = result?;
            if !status.success() {
                let code = status.code().unwrap_or(-1);
                bail!("Flux server exited with status {}", code);
            }
        }
        _ = signal::ctrl_c() => {
            println!();
            println!("{}", "Stopping…".dimmed());
            let _ = server.kill().await;
            let _ = server.wait().await;
        }
    }

    // 7. Stop embedded PostgreSQL if we started it
    println!("{}", "Stopping PostgreSQL…".dimmed());
    if let Some(mut pg) = _pg {
        let _ = pg.stop().await;
    }
    println!("{}", "✔  Stopped.".green());

    Ok(())
}

// ── PostgreSQL ────────────────────────────────────────────────────────────────

/// Resolve `DATABASE_URL` and optionally start embedded Postgres.
///
/// Returns `(url, Some(pg))` when embedded Postgres was started.
/// Returns `(url, None)` when using an external DATABASE_URL.
///
/// Priority:
///   1. `DATABASE_URL` shell environment variable
///   2. `DATABASE_URL` in `.env` file at project root
///   3. Embedded Postgres (auto-start, zero-config)
async fn resolve_database(
    project_root: &Path,
) -> anyhow::Result<(String, Option<postgresql_embedded::PostgreSQL>)> {
    // 1. Shell env var (highest priority — overrides everything)
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.is_empty() {
            println!(
                "  {} database        {}",
                "✔".green().bold(),
                "(DATABASE_URL from environment)".dimmed()
            );
            return Ok((url, None));
        }
    }

    // 2. .env file at project root
    let dot_env_path = project_root.join(".env");
    if dot_env_path.exists() {
        // dotenvy::from_path_iter gives us key-value pairs without mutating
        // the process environment (we only want DATABASE_URL, not everything).
        if let Ok(iter) = dotenvy::from_path_iter(&dot_env_path) {
            for item in iter.flatten() {
                if item.0 == "DATABASE_URL" && !item.1.is_empty() {
                    println!(
                        "  {} database        {}",
                        "✔".green().bold(),
                        "(.env DATABASE_URL)".dimmed()
                    );
                    return Ok((item.1, None));
                }
            }
        }
    }

    // 3. Embedded Postgres — zero-config default
    let (pg, port) = start_postgres(project_root).await?;
    let url = format!(
        "postgres://{}:{}@localhost:{}/{}",
        DEV_DB_USER, DEV_DB_PASS, port, DEV_DB_NAME
    );
    Ok((url, Some(pg)))
}

async fn start_postgres(project_root: &Path) -> anyhow::Result<(postgresql_embedded::PostgreSQL, u16)> {
    use postgresql_embedded::{PostgreSQL, Settings};

    // Per-project data directory — each project has completely isolated state.
    let data_dir = project_root.join(".flux").join("dev").join("pgdata");
    std::fs::create_dir_all(&data_dir)
        .context("Failed to create .flux/dev/pgdata/")?;

    // Resolve a stable per-project port. Written to .flux/dev/port on first
    // run so every subsequent `flux dev` in this project uses the same port.
    // This means DATABASE_URL never changes, and multiple projects running
    // simultaneously never fight over the same port.
    let port = resolve_db_port(project_root)?;

    // Cache directory for the downloaded Postgres binary — shared across all projects.
    let cache_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("cache")
        .join("postgres");
    std::fs::create_dir_all(&cache_dir)
        .context("Failed to create ~/.flux/cache/postgres/")?;

    print!("  {} PostgreSQL      downloading if needed… ", "↓".blue().bold());
    // Flush so the message appears before the potentially slow setup() call.
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let settings = Settings {
        username:         DEV_DB_USER.into(),
        password:         DEV_DB_PASS.into(),
        data_dir:         data_dir.clone(),
        port,
        temporary:        false,
        installation_dir: cache_dir,
        ..Default::default()
    };

    let mut pg = PostgreSQL::new(settings);

    // setup() downloads the binary on first run (cached after that).
    pg.setup().await.context("Failed to set up embedded PostgreSQL")?;
    pg.start().await.context("Failed to start embedded PostgreSQL")?;

    // Create the dev database if it doesn't exist yet.
    if !pg.database_exists(DEV_DB_NAME).await.unwrap_or(false) {
        pg.create_database(DEV_DB_NAME).await
            .context("Failed to create dev database")?;
    }

    println!("\r  {} postgres        localhost:{}         ",
        "✔".green().bold(),
        port.to_string().cyan(),
    );

    Ok((pg, port))
}

// ── Migrations ────────────────────────────────────────────────────────────────

async fn run_migrations(database_url: &str, project_root: &Path) -> anyhow::Result<usize> {
    use sqlx::postgres::PgPoolOptions;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
        .context("Failed to connect to dev PostgreSQL")?;

    let mut total = 0usize;

    // Apply both migration sets in order. sqlx migrate tracks applied migrations
    // in a `_sqlx_migrations` table — safe to call multiple times.
    let api_dir = project_root.join("schemas").join("api");
    let de_dir  = project_root.join("schemas").join("data-engine");

    for dir in [&api_dir, &de_dir] {
        if dir.exists() {
            let migrator = sqlx::migrate::Migrator::new(dir.as_path())
                .await
                .with_context(|| format!("Failed to load migrations from {:?}", dir))?;
            migrator.run(&pool).await
                .with_context(|| format!("Failed to run migrations from {:?}", dir))?;
            total += migrator.migrations.len();
        }
    }

    pool.close().await;
    Ok(total)
}

// ── Server ────────────────────────────────────────────────────────────────────

async fn start_server(port: u16, database_url: &str) -> anyhow::Result<tokio::process::Child> {
    let binary = find_server_binary()
        .ok_or_else(|| anyhow::anyhow!(
            "Flux server binary not found.\n  Run `cargo build` first, or install Flux."
        ))?;

    print!("  {} flux server     starting… ", "◆".blue());
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let child = Command::new(&binary)
        .env("PORT", port.to_string())
        .env("DATABASE_URL", database_url)
        .env("FLUX_LOCAL", "true")
        .env("LOCAL_MODE", "true")
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to start server binary: {:?}", binary))?;

    // Health-check until the server is ready.
    let ready = wait_healthy(&format!("http://localhost:{}", port), "/health", 30).await;

    if ready {
        println!("\r  {} flux server     localhost:{}         ",
            "✔".green().bold(),
            port.to_string().cyan(),
        );
    } else {
        println!("\r  {} flux server     localhost:{} (still starting…)",
            "⚠".yellow().bold(),
            port.to_string().cyan(),
        );
    }

    Ok(child)
}

// ── Banner ────────────────────────────────────────────────────────────────────

fn print_banner(port: u16) {
    println!();
    println!("  {}  http://localhost:{}", "Flux  ".bold(), port.to_string().cyan().bold());
    println!("  {}  http://localhost:{}/flux/api", "API   ".bold(), port.to_string().cyan());
    println!("  {}  http://localhost:{}/flux", "Dash  ".bold(), port.to_string().cyan());
    println!();
    println!("  {}  — call a function",   "flux invoke <fn>  ".cyan());
    println!("  {}  — inspect a request", "flux trace <id>   ".cyan());
    println!("  {}  — root-cause error",  "flux why <id>     ".cyan());
    println!("  {}  — regenerate types",  "flux generate     ".cyan());
    println!();
    println!("{}", "  Press Ctrl+C to stop.".dimmed());
    println!();
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return a stable per-project Postgres port.
///
/// On first run: scan for a free port starting at `DEV_DB_PORT_START`,
/// write it to `.flux/dev/port`, return it.
///
/// On subsequent runs: read the stored port and return it unchanged.
/// This guarantees DATABASE_URL is stable across restarts and that two
/// projects running simultaneously never bind the same port.
fn resolve_db_port(project_root: &Path) -> anyhow::Result<u16> {
    let port_file = project_root.join(".flux").join("dev").join("port");

    // If we already assigned a port for this project, reuse it.
    if port_file.exists() {
        let raw = std::fs::read_to_string(&port_file)
            .context("Failed to read .flux/dev/port")?;
        if let Ok(p) = raw.trim().parse::<u16>() {
            return Ok(p);
        }
    }

    // First run — find a free port.
    let port = find_free_port(DEV_DB_PORT_START)
        .ok_or_else(|| anyhow::anyhow!("No free port available in range 5433–5600"))?;

    // Persist so every subsequent run uses the same port.
    if let Some(parent) = port_file.parent() {
        std::fs::create_dir_all(parent).context("Failed to create .flux/dev/")?;
    }
    std::fs::write(&port_file, port.to_string())
        .context("Failed to write .flux/dev/port")?;

    Ok(port)
}

/// Scan from `start` upward to find a TCP port not currently in use.
fn find_free_port(start: u16) -> Option<u16> {
    use std::net::TcpListener;
    (start..5600).find(|&p| TcpListener::bind(("127.0.0.1", p)).is_ok())
}

/// Walk upward from cwd to find a directory containing `schemas/`, `.flux/`,
/// or `flux.toml` — that's the project root.
fn find_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("schemas").exists()
            || dir.join(".flux").exists()
            || dir.join("flux.toml").exists()
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Public wrapper used by other CLI commands (e.g. `db_push`).
pub fn find_project_root_pub() -> Option<PathBuf> {
    find_project_root()
}

/// Resolve the `server` binary: alongside the flux binary, then
/// workspace target/debug or target/release, then PATH.
fn find_server_binary() -> Option<PathBuf> {
    // 1. Alongside the flux binary (distribution layout)
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent()?.join(if cfg!(windows) { "server.exe" } else { "server" });
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2. Workspace target/debug or target/release
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let debug   = dir.join("target").join("debug")
            .join(if cfg!(windows) { "server.exe" } else { "server" });
        let release = dir.join("target").join("release")
            .join(if cfg!(windows) { "server.exe" } else { "server" });
        if debug.exists()   { return Some(debug);   }
        if release.exists() { return Some(release); }
        if dir.join("Cargo.toml").exists() { break; }
        if !dir.pop() { break; }
    }

    // 3. PATH
    which::which("server").ok()
}

/// Poll `{base_url}{path}` until 2xx or timeout.
async fn wait_healthy(base_url: &str, path: &str, timeout_secs: u64) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    let url      = format!("{}{}", base_url, path);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        if tokio::time::Instant::now() >= deadline { return false; }
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() { return true; }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

