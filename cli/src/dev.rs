//! `flux dev` — local development server.
//!
//! Starts the full Flux stack. Requires `DATABASE_URL` to be set.
//!
//! ```text
//! $ flux dev
//!
//! ◆ Starting Flux dev server…
//!
//!   ✔ database        (DATABASE_URL from environment)
//!   ✔ migrations     1 applied
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
//! 1. DATABASE_URL is resolved from environment or .env file.
//!    If not set, fails fast with a helpful error message.
//! 2. The canonical `schemas/v0.1.sql` baseline is applied (idempotent).
//! 3. The Flux server binary is started.
//! 4. Ctrl+C stops the server process.
//!
//! ## Quick start
//!
//! ```bash
//! docker run -p 5432:5432 -e POSTGRES_PASSWORD=flux postgres:16
//! export DATABASE_URL=postgres://postgres:flux@localhost/flux
//! flux dev
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

use anyhow::Context;
use colored::Colorize;
use serde::Deserialize;
use tokio::process::Command;
use tokio::signal;

use api_contract::routes as R;
use crate::config::DEFAULT_SERVER_PORT;

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute() -> anyhow::Result<()> {
    println!();
    println!("{}", "◆ Starting Flux dev server…".cyan().bold());
    println!();

    // 1. Locate project root for relative paths (schemas/, .flux/)
    let project_root = find_project_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // 2. Prepare the build directory for function bundles.
    //    The runtime reads bundles from this directory (FLUX_FUNCTIONS_DIR).
    let build_dir = project_root.join(".flux").join("build");
    std::fs::create_dir_all(&build_dir)
        .with_context(|| format!("cannot create build dir: {}", build_dir.display()))?;

    // 3. Load all vars from .env file (skipping DATABASE_URL which is handled below).
    //    These are injected into the dev server process so they become available
    //    in ctx.secrets / ctx.env without needing a real secrets backend.
    let dot_env_vars = load_dot_env(&project_root);
    if !dot_env_vars.is_empty() {
        println!(
            "  {} .env            {} var{}",
            "✔".green().bold(),
            dot_env_vars.len().to_string().cyan(),
            if dot_env_vars.len() == 1 { "" } else { "s" }
        );
    }

    // 4. Resolve DATABASE_URL — check env var, then .env file, fail fast if missing.
    let database_url = resolve_database(&project_root).await?;

    // 5. Run all migrations
    let migration_count = run_migrations(&database_url, &project_root).await?;
    println!(
        "  {} migrations     {} applied",
        "✔".green().bold(),
        migration_count.to_string().cyan(),
    );

    // 6. Start Flux server — pass FLUX_FUNCTIONS_DIR so the runtime reads
    //    bundles from the local build directory instead of Postgres.
    //    Scan for a free port from DEFAULT_SERVER_PORT upward so multiple
    //    projects can run simultaneously without clashing.
    let server_port = find_free_server_port(DEFAULT_SERVER_PORT)
        .ok_or_else(|| anyhow::anyhow!("No free port available in range {}–{}", DEFAULT_SERVER_PORT, DEFAULT_SERVER_PORT + 100))?;
    let mut server = start_server(server_port, &database_url, &dot_env_vars, &build_dir).await?;

    // 7. Start file watcher for hot-reload.
    //    Watches functions/ directory; on change rebuilds the affected bundle,
    //    writes it to .flux/build/, and invalidates the runtime cache.
    //
    //    The watcher loop uses a BLOCKING std::sync::mpsc::recv_timeout — it
    //    MUST run on a real OS thread, not a tokio::spawn task.  tokio can only
    //    cancel tasks at .await points; a blocking recv_timeout has none, so
    //    tokio::spawn + .abort() leaves the thread blocked and the runtime
    //    hangs on shutdown.  We use an Arc<AtomicBool> cancel flag instead:
    //    the loop checks it each iteration and exits within one 100 ms window.
    let functions_src = project_root.join("functions");
    let watcher_cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let watcher_thread: Option<std::thread::JoinHandle<()>> = if functions_src.exists() {
        let build_dir_clone = build_dir.clone();
        let functions_src_clone = functions_src.clone();
        let cancel_flag = Arc::clone(&watcher_cancel);
        let handle = std::thread::spawn(move || {
            if let Err(e) = watch_functions_sync(&functions_src_clone, &build_dir_clone, server_port, &cancel_flag) {
                eprintln!("file watcher error: {e}");
            }
        });
        println!(
            "  {} watcher         watching {}",
            "✔".green().bold(),
            "functions/".cyan(),
        );
        Some(handle)
    } else {
        None
    };

    // 8. First-run: if no admin account exists, prompt to create one.
    prompt_admin_setup_if_needed(server_port).await;

    // 9. Print banner
    print_banner(server_port);

    // 10. Wait for Ctrl+C or server exit
    let server_result = tokio::select! {
        result = server.wait() => {
            let status = result?;
            if !status.success() {
                let code = status.code().unwrap_or(-1);
                Err(anyhow::anyhow!("Flux server exited with status {}", code))
            } else {
                Ok(())
            }
        }
        _ = signal::ctrl_c() => {
            println!();
            println!("{}", "Stopping…".dimmed());
            // SIGTERM first; give 3 s before SIGKILL.
            #[cfg(unix)]
            if let Some(pid) = server.id() {
                let _ = std::process::Command::new("kill")
                    .args(["-s", "TERM", &pid.to_string()])
                    .status();
            }
            tokio::select! {
                _ = server.wait() => {}
                _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {
                    let _ = server.kill().await;
                    let _ = server.wait().await;
                }
            }
            Ok(())
        }
    };

    // 11. Stop the file watcher thread.
    //     Set the cancel flag — the thread checks it each recv_timeout loop
    //     (100 ms window) and exits promptly.  We don't join; process::exit
    //     below will reap it along with everything else.
    watcher_cancel.store(true, Ordering::Relaxed);
    drop(watcher_thread); // detach — join would block up to 100 ms unnecessarily

    println!("{}", "✔  Stopped.".green());

    // Force-exit immediately. Without this, dropping the tokio runtime here
    // would block waiting for the watcher OS thread (and any other background
    // work), keeping the terminal unresponsive after the shutdown messages.
    // All cleanup (server kill) is already done above.
    let exit_code: i32 = match server_result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{} {e}", "error:".red().bold());
            1
        }
    };
    std::process::exit(exit_code);
}

/// Load all key-value pairs from the `.env` file at `project_root`.
///
/// - Skips `DATABASE_URL` (handled separately by `resolve_database`).
/// - Skips lines that fail to parse (malformed entries don't abort startup).
/// - Returns an empty vec if the file doesn't exist.
///
/// Uses `dotenvy` for correct handling of quotes, escapes, and comments.
fn load_dot_env(project_root: &Path) -> Vec<(String, String)> {
    let path = project_root.join(".env");
    if !path.exists() {
        return vec![];
    }
    let Ok(iter) = dotenvy::from_path_iter(&path) else {
        return vec![];
    };
    iter.flatten()
        .filter(|(k, _)| k != "DATABASE_URL")
        .collect()
}

// ── PostgreSQL ────────────────────────────────────────────────────────────────

async fn resolve_database(
    project_root: &Path,
) -> anyhow::Result<String> {
    // 1. Shell env var (highest priority — overrides everything)
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.is_empty() {
            println!(
                "  {} database        {}",
                "✔".green().bold(),
                "(DATABASE_URL from environment)".dimmed()
            );
            return Ok(url);
        }
    }

    // 2. .env file at project root
    let dot_env_path = project_root.join(".env");
    if dot_env_path.exists() {
        if let Ok(iter) = dotenvy::from_path_iter(&dot_env_path) {
            for item in iter.flatten() {
                if item.0 == "DATABASE_URL" && !item.1.is_empty() {
                    println!(
                        "  {} database        {}",
                        "✔".green().bold(),
                        "(.env DATABASE_URL)".dimmed()
                    );
                    return Ok(item.1);
                }
            }
        }
    }

    // Neither env var nor .env — fail fast with a helpful message.
    anyhow::bail!(
        "\n{}\n\nFlux requires a Postgres database. Set DATABASE_URL and retry.\n\nQuick start:\n  {}\n  {}\n  {}\n",
        "ERROR: DATABASE_URL is not set.".red().bold(),
        "docker run -p 5432:5432 -e POSTGRES_PASSWORD=flux postgres:16".cyan(),
        "export DATABASE_URL=postgres://postgres:flux@localhost/flux".cyan(),
        "flux dev".cyan(),
    )
}

// ── Migrations ────────────────────────────────────────────────────────────────

// Canonical schema baseline embedded at compile-time — always available
// regardless of where the CLI is installed. All DDL uses CREATE … IF NOT EXISTS
// and DROP … IF EXISTS guards so this is safe to run on existing databases.
static SCHEMA_V01: &str = include_str!("../../schemas/v0.1.sql");

async fn run_migrations(database_url: &str, _project_root: &Path) -> anyhow::Result<usize> {
    use sqlx::postgres::PgPoolOptions;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await
        .context("Failed to connect to dev PostgreSQL")?;

    sqlx::raw_sql(SCHEMA_V01)
        .execute(&pool)
        .await
        .context("Failed to apply Flux v0.1 schema")?;

    pool.close().await;
    Ok(1)
}

// ── Server ────────────────────────────────────────────────────────────────────

async fn start_server(port: u16, database_url: &str, extra_env: &[(String, String)], build_dir: &Path) -> anyhow::Result<tokio::process::Child> {
    let binary = find_server_binary()
        .ok_or_else(|| anyhow::anyhow!(
            "Flux server binary not found.\n  Run `cargo build` first, or install Flux."
        ))?;

    print!("  {} flux server     starting… ", "◆".blue());
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let mut cmd = Command::new(&binary);
    cmd.env("PORT", port.to_string())
        .env("DATABASE_URL", database_url)
        .env("FLUX_LOCAL", "true")
        .env("LOCAL_MODE", "true")
        .env("RUST_LOG", "warn")
        // Bundles live in the local build dir; runtime reads from here instead of DB.
        .env("FLUX_FUNCTIONS_DIR", build_dir.as_os_str())
        // Do not inherit stdin — the server doesn't read from it, and leaving
        // stdin shared with the CLI process can leave the terminal in a broken
        // state when the server exits.
        .stdin(std::process::Stdio::null());

    // Resolve the dashboard static-export directory so the server can find it
    // regardless of the CWD the user runs `flux dev` from.
    if let Some(dashboard) = find_dashboard_dir(&binary) {
        cmd.env("FLUX_DASHBOARD_DIR", dashboard);
    }

    // Inject all .env vars so they are available in ctx.secrets / ctx.env
    // inside functions without needing a real secrets backend.
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let child = cmd
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to start server binary: {:?}", binary))?;

    // Health-check until the server is ready.
    let ready = wait_healthy(&format!("http://localhost:{}", port), R::health::HEALTH.path, 30).await;

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

// ── First-run admin setup ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AuthStatus { user_count: u64 }

/// After the server starts, silently check if any admin account exists.
/// If not, print a one-time prompt so the developer can immediately log into
/// the dashboard without having to remember a separate CLI command.
async fn prompt_admin_setup_if_needed(port: u16) {
    let base = format!("http://localhost:{}/flux/api", port);
    let Ok(res) = reqwest::get(R::auth::STATUS.url(&base)).await else { return };
    let Ok(status) = res.json::<AuthStatus>().await else { return };
    if status.user_count > 0 { return; }   // already set up

    println!();
    println!("  {} No admin account found.", "→".cyan().bold());
    println!("  Run {} to create one and open the dashboard:", "flux login".cyan().bold());
    println!("  Or call: {} to do it now? [y/N] ", "admin setup".cyan());

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y") {
        if let Err(e) = crate::auth::execute().await {
            println!("  {} {}", "✗".red(), e);
        }
    } else {
        println!("  Run {} whenever you're ready.\n", "flux login".cyan().bold());
    }
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

/// Find a free port for the Flux server, starting from `start` (default 4000).
/// Tries up to 100 ports before giving up.
///
/// Must check `0.0.0.0` (not `127.0.0.1`) because the server binds the
/// wildcard address. On macOS a loopback-only bind can succeed even when
/// `0.0.0.0:port` is already held by a prior server process, causing the
/// new server to fail with EADDRINUSE at startup.
fn find_free_server_port(start: u16) -> Option<u16> {
    use std::net::TcpListener;
    (start..start.saturating_add(100)).find(|&p| TcpListener::bind(("0.0.0.0", p)).is_ok())
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

/// Resolve the `dashboard/out` directory given the resolved server binary path.
///
/// Layout 1 — workspace build (`target/debug/server` or `target/release/server`):
///   binary = {workspace}/target/{profile}/server
///   out    = {workspace}/dashboard/out
///
/// Layout 2 — distribution install (all binaries alongside each other):
///   binary = {prefix}/bin/server
///   out    = {prefix}/share/flux/dashboard/out   (or sibling `dashboard/out`)
///
/// Layout 3 — cargo install to ~/.cargo/bin or /usr/local/bin:
///   Walk up from the CLI binary itself and from CWD to find a workspace
///   with a `dashboard/out/index.html`.
///
/// Falls back gracefully — if nothing is found, returns None and the server
/// will use its own "dashboard/out" relative-path default.
fn find_dashboard_dir(server_binary: &Path) -> Option<PathBuf> {
    // Helper: check if a directory looks like a valid dashboard export.
    let valid = |p: &PathBuf| p.join("index.html").exists();

    // Layout 1: workspace — server binary sits under target/{profile}/
    if let Some(workspace) = server_binary
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        let candidate = workspace.join("dashboard").join("out");
        if valid(&candidate) { return Some(candidate); }
    }

    // Layout 2: distribution — look for `dashboard/out` next to the binary
    if let Some(bin_dir) = server_binary.parent() {
        let candidate = bin_dir.join("dashboard").join("out");
        if valid(&candidate) { return Some(candidate); }
        // FHS-style: {prefix}/share/flux/dashboard/out
        if let Some(prefix) = bin_dir.parent() {
            let candidate = prefix.join("share").join("flux").join("dashboard").join("out");
            if valid(&candidate) { return Some(candidate); }
        }
    }

    // Layout 3: cargo install layout (~/.cargo/bin/server, /usr/local/bin/server).
    // Walk up from the CLI binary itself to find the workspace.
    if let Ok(cli_exe) = std::env::current_exe() {
        let mut dir = cli_exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        loop {
            let candidate = dir.join("dashboard").join("out");
            if valid(&candidate) { return Some(candidate); }
            // Stop at a clear workspace root (has Cargo.toml + dashboard/).
            if dir.join("Cargo.toml").exists() { break; }
            if !dir.pop() { break; }
        }
    }

    // Layout 4: walk up from CWD (useful for `cargo run -p cli -- dev`).
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let candidate = dir.join("dashboard").join("out");
            if valid(&candidate) { return Some(candidate); }
            if dir.join("Cargo.toml").exists() { break; }
            if !dir.pop() { break; }
        }
    }

    // Layout 5: compile-time workspace path (reliable for local `cargo install`).
    // CARGO_MANIFEST_DIR is set by cargo to the cli/ directory at compile time,
    // so `../dashboard/out` resolves to the actual workspace dashboard export.
    // This won't exist in distribution packages, where layouts 1-2 succeed first.
    let compile_time_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|workspace| workspace.join("dashboard").join("out"));
    if let Some(c) = compile_time_candidate {
        if valid(&c) { return Some(c); }
    }

    None
}

/// Watch `functions/` for source file changes. On any write/create event:
///  1. Determine which function changed (parent directory name)
///  2. Re-bundle via the same logic used by `flux deploy`
///  3. Write the result to `{build_dir}/{name}.js|.wasm`
///  4. POST cache invalidation to the local dev server so the new bundle is picked up
///
/// Debounces events with a 200 ms window to avoid double-rebuilds on editor saves.
///
/// Runs on a real OS thread (NOT a tokio task) because the inner loop uses
/// std::sync::mpsc::recv_timeout which is a blocking call with no .await points.
/// The caller passes a cancel flag; the loop exits within one 100 ms window of
/// the flag being set.
fn watch_functions_sync(functions_dir: &Path, build_dir: &Path, port: u16, cancel: &AtomicBool) -> anyhow::Result<()> {
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(functions_dir, RecursiveMode::Recursive)?;

    // Per-function debounce: track last rebuild time.
    let mut last_built: HashMap<String, Instant> = HashMap::new();
    let debounce = Duration::from_millis(200);

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                let is_write = matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                );
                if !is_write { continue; }

                for path in &event.paths {
                    // Ignore build outputs inside functions/<name>/dist/ — those are written by
                    // bundle_js itself and would cause an infinite rebuild loop.
                    if path.components().any(|c| c.as_os_str() == "dist" || c.as_os_str() == "node_modules") {
                        continue;
                    }

                    // Derive function name from the immediate child dir of functions_dir.
                    let func_name = path
                        .strip_prefix(functions_dir)
                        .ok()
                        .and_then(|rel| rel.components().next())
                        .and_then(|c| c.as_os_str().to_str())
                        .map(str::to_owned);

                    let name = match func_name {
                        Some(n) => n,
                        None => continue,
                    };

                    // Debounce: skip if rebuilt within the last 200 ms.
                    let now = Instant::now();
                    if let Some(last) = last_built.get(&name) {
                        if now.duration_since(*last) < debounce { continue; }
                    }
                    last_built.insert(name.clone(), now);

                    let func_dir = functions_dir.join(&name);
                    let flux_json = func_dir.join("flux.json");
                    if !flux_json.exists() { continue; }

                    let metadata: serde_json::Value = match std::fs::read_to_string(&flux_json)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                    {
                        Some(v) => v,
                        None => continue,
                    };

                    match crate::deploy::bundle_function(&func_dir, &metadata) {
                        Ok(bundle) => {
                            let ext = if bundle.runtime == "wasm" { "wasm" } else { "js" };
                            let dest = build_dir.join(format!("{name}.{ext}"));
                            if std::fs::write(&dest, &bundle.bytes).is_ok() {
                                // Invalidate the runtime cache for this function (best-effort).
                                let api_base = format!("http://localhost:{port}/flux/api");
                                let url = R::internal::CACHE_INVALIDATE.url(&api_base);
                                let body = serde_json::json!({ "function_id": name });
                                let _ = reqwest::blocking::Client::new()
                                    .post(&url)
                                    .header("X-Service-Token", "dev-service-token")
                                    .json(&body)
                                    .send();
                                println!(
                                    "  {} rebuilt         {}",
                                    "↺".cyan().bold(),
                                    name.cyan()
                                );
                            }
                        }
                        Err(e) => {
                            println!(
                                "  {} rebuild failed  {} — {e}",
                                "✘".red().bold(),
                                name.red()
                            );
                        }
                    }
                }
            }
            Ok(Err(e)) => eprintln!("watch error: {e}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

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

