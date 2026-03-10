//! `flux stack` — local Fluxbase development stack manager.
//!
//! Wraps `docker compose` to start, stop, inspect, and tail logs for the full
//! Fluxbase development stack defined in `docker-compose.dev.yml`.
//!
//! ```text
//! $ flux stack up            — build and start all services (detached)
//! $ flux stack up --build    — force rebuild images first
//! $ flux stack down          — stop and remove containers
//! $ flux stack down -v       — also remove the postgres volume
//! $ flux stack ps            — list running services + ports
//! $ flux stack logs          — tail all services
//! $ flux stack logs api      — tail a single service
//! ```
//!
//! On first run, copy `.env.dev.example` → `.env.dev` and set
//! `FIREBASE_PROJECT_ID`.  The CLI auto-loads `.env.dev` when present.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use colored::Colorize;

const COMPOSE_FILE: &str = "docker-compose.dev.yml";
const ENV_FILE:     &str = ".env.dev";

// ─── Subcommand enum  (inlined into main.rs via clap subcommand) ──────────────

/// Find docker-compose.dev.yml walking upward from cwd (like git finds .git).
fn find_compose_file() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(COMPOSE_FILE);
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Build the base `docker compose` invocation with the correct -f and optional --env-file.
fn base_args(compose_path: &Path) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string_lossy().into_owned(),
    ];

    // Auto-inject .env.dev when it exists alongside the compose file.
    if let Some(parent) = compose_path.parent() {
        let env_path = parent.join(ENV_FILE);
        if env_path.exists() {
            args.push("--env-file".to_string());
            args.push(env_path.to_string_lossy().into_owned());
        } else {
            eprintln!(
                "{} {} not found — copy {} to set FIREBASE_PROJECT_ID",
                "⚠".yellow().bold(),
                ENV_FILE.cyan(),
                ".env.dev.example".cyan(),
            );
        }
    }

    args
}

/// Run a docker command, streaming stdout/stderr directly to the terminal.
fn run(args: Vec<String>) -> anyhow::Result<()> {
    let mut cmd = Command::new("docker");
    for a in &args {
        cmd.arg(a);
    }
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("'docker' not found — install Docker Desktop or Docker Engine first")
        } else {
            anyhow::anyhow!("docker error: {}", e)
        }
    })?;

    if !status.success() {
        anyhow::bail!("docker compose exited with status {}", status);
    }
    Ok(())
}

// ─── Command handlers ─────────────────────────────────────────────────────────

pub async fn execute_up(build: bool, detach: bool) -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!(
            "{} not found.\n  Run from the Fluxbase project root or a subdirectory.",
            COMPOSE_FILE
        )
    })?;

    let mut args = base_args(&compose);
    args.push("up".to_string());
    if build   { args.push("--build".to_string()); }
    if detach  { args.push("-d".to_string()); }

    println!(
        "{} Starting Fluxbase stack  {}",
        "▶".green().bold(),
        compose.display().to_string().dimmed(),
    );
    println!("{}", "  Services: db · api · gateway · data-engine · runtime · queue · dashboard".dimmed());
    println!();

    run(args)
}

pub async fn execute_down(volumes: bool) -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!("{} not found.", COMPOSE_FILE)
    })?;

    let mut args = base_args(&compose);
    args.push("down".to_string());
    if volumes { args.push("-v".to_string()); }

    println!("{} Stopping Fluxbase stack…", "■".red().bold());
    run(args)
}

pub async fn execute_ps() -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!("{} not found.", COMPOSE_FILE)
    })?;

    let mut args = base_args(&compose);
    args.push("ps".to_string());
    run(args)
}

pub async fn execute_logs(service: Option<String>, tail: u32) -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!("{} not found.", COMPOSE_FILE)
    })?;

    let mut args = base_args(&compose);
    args.push("logs".to_string());
    args.push("-f".to_string());
    args.push("--tail".to_string());
    args.push(tail.to_string());

    if let Some(svc) = service {
        args.push(svc);
    }

    run(args)
}

/// `flux stack reset` — stop the stack, wipe volumes, then restart.
pub async fn execute_reset() -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!("{} not found.", COMPOSE_FILE)
    })?;

    // Prompt for confirmation before wiping data
    eprint!(
        "{} This will {}. Continue? [y/N] ",
        "⚠".yellow().bold(),
        "destroy all local data volumes".red().bold(),
    );
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(std::io::stdin())
        .read_line(&mut line)?;
    if !matches!(line.trim(), "y" | "Y") {
        println!("Aborted.");
        return Ok(());
    }

    println!("{} Stopping services and removing volumes…", "■".red().bold());
    let mut down_args = base_args(&compose);
    down_args.extend(["down".to_string(), "-v".to_string()]);
    run(down_args)?;

    println!("{} Restarting Fluxbase stack…", "▶".green().bold());
    let mut up_args = base_args(&compose);
    up_args.extend(["up".to_string(), "--build".to_string(), "-d".to_string()]);
    run(up_args)
}

/// `flux stack seed` — run the seed script inside the running `api` container.
///
/// Looks for `scripts/seed.sql` or `scripts/seed.sh` relative to the compose
/// file, then executes it inside the `db` service.
pub async fn execute_seed(seed_file: Option<String>) -> anyhow::Result<()> {
    let compose = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!("{} not found.", COMPOSE_FILE)
    })?;
    let root = compose.parent().unwrap_or(std::path::Path::new("."));

    // Resolve seed file path
    let seed = if let Some(f) = seed_file {
        std::path::PathBuf::from(f)
    } else {
        let sql = root.join("scripts/seed.sql");
        let sh  = root.join("scripts/seed.sh");
        if sql.exists() {
            sql
        } else if sh.exists() {
            sh
        } else {
            anyhow::bail!(
                "No seed file found. Expected scripts/seed.sql or scripts/seed.sh.\n\
                 Pass --file <path> to specify one."
            );
        }
    };

    println!(
        "{} Seeding from {}…",
        "▶".green().bold(),
        seed.display().to_string().cyan()
    );

    // Pipe the file through docker exec into psql inside the db container
    let mut base = base_args(&compose);
    base.extend([
        "exec".to_string(),
        "-T".to_string(),
        "db".to_string(),
        "psql".to_string(),
        "-U".to_string(),
        "postgres".to_string(),
    ]);

    let file_content = std::fs::read(&seed)
        .map_err(|e| anyhow::anyhow!("Cannot read {}: {}", seed.display(), e))?;

    use std::io::Write;
    let mut cmd = std::process::Command::new("docker");
    for a in &base { cmd.arg(a); }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("docker error: {}", e))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(&file_content)?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("Seed command exited with status {}", status);
    }
    println!("{} Seed complete.", "✔".green().bold());
    Ok(())
}
