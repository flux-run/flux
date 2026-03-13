//! `flux dev` — start the full Flux development stack locally.
//!
//! ```text
//! $ flux dev
//!
//! ▶  Starting Flux dev stack …
//!
//!    db            postgres     :5432
//!    flux          server       :4000   LOCAL_MODE=true
//!    dashboard     spa          :5173
//!
//! ✔  Flux is running.
//!
//!    Flux  http://localhost:4000
//!    API   http://localhost:4000/flux/api
//!    Dash  http://localhost:5173/flux
//!
//!    flux invoke <fn>        — call a function
//!    flux deploy             — deploy changed functions
//!    flux trace <id>         — inspect a request
//!
//!    Press Ctrl+C to stop.
//! ```
//!
//! Wraps `docker compose -f docker-compose.dev.yml up` with `LOCAL_MODE=true`
//! so the gateway skips tenant auth in local dev.

use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;
use tokio::process::Command;
use tokio::signal;

use crate::config::{FluxToml, DEFAULT_SERVER_PORT, DEFAULT_DASHBOARD_PORT, DEFAULT_DB_PORT};

const COMPOSE_FILE: &str = "docker-compose.dev.yml";
const ENV_FILE:     &str = ".env.dev";

// ── Service descriptor ────────────────────────────────────────────────────────

#[derive(Clone)]
struct ServiceInfo {
    name:       &'static str,
    label:      &'static str,
    port:       u16,
    local_mode: bool,
}

fn default_services() -> Vec<ServiceInfo> {
    vec![
        ServiceInfo { name: "db",        label: "postgres", port: DEFAULT_DB_PORT,        local_mode: false },
        ServiceInfo { name: "flux",      label: "server",   port: DEFAULT_SERVER_PORT,    local_mode: true  },
        ServiceInfo { name: "dashboard", label: "spa",      port: DEFAULT_DASHBOARD_PORT, local_mode: false },
    ]
}

// ── Filesystem helpers ────────────────────────────────────────────────────────

/// Walk upward from cwd looking for `docker-compose.dev.yml`, like git finds `.git`.
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

/// Build the base `docker compose` argument list.
fn compose_args(compose_path: &Path) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string_lossy().into_owned(),
    ];
    // Inject .env.dev when it exists alongside the compose file.
    if let Some(parent) = compose_path.parent() {
        let env_path = parent.join(ENV_FILE);
        if env_path.exists() {
            args.push("--env-file".to_string());
            args.push(env_path.to_string_lossy().into_owned());
        }
    }
    args
}

// ── Health check ──────────────────────────────────────────────────────────────

/// Poll `base_url + path` until an HTTP 2xx arrives or `timeout_secs` elapses.
/// Returns `true` on success, `false` on timeout.
async fn wait_healthy(base_url: &str, path: &str, timeout_secs: u64) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    let url      = format!("{}{}", base_url, path);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => return true,
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub async fn execute() -> anyhow::Result<()> {
    // ── Prereq checks ────────────────────────────────────────────────────────
    if which::which("docker").is_err() {
        anyhow::bail!(
            "'docker' not found.\n  Install Docker Desktop: https://docs.docker.com/desktop/"
        );
    }

    let compose_path = find_compose_file().ok_or_else(|| {
        anyhow::anyhow!(
            "'{}' not found.\n  Run `flux dev` from your project root.",
            COMPOSE_FILE
        )
    })?;

    // ── Apply flux.toml [dev] port overrides ─────────────────────────────────
    let mut services = default_services();
    if let Some(t) = FluxToml::load_sync() {
        for svc in services.iter_mut() {
            let override_port = match svc.name {
                "flux" => t.dev.gateway_port,  // single server reuses gateway_port override
                _      => None,
            };
            if let Some(p) = override_port {
                svc.port = p;
            }
        }
    }

    // ── Print service table ──────────────────────────────────────────────────
    println!();
    println!("{}", "\u{25b6}  Starting Flux dev stack \u{2026}".cyan().bold());
    println!();

    for svc in &services {
        let badge = if svc.local_mode {
            format!("  {}", "LOCAL_MODE=true".yellow())
        } else {
            String::new()
        };
        println!(
            "   {:<14}  {:<12}  :{}{}",
            svc.name.bold(),
            svc.label.dimmed(),
            svc.port.to_string().cyan(),
            badge,
        );
    }
    println!();

    // Warn if .env.dev is missing.
    if let Some(parent) = compose_path.parent() {
        if !parent.join(ENV_FILE).exists() {
            println!(
                "{} {} not found \u{2014} copy {} to configure secrets",
                "\u{26a0}".yellow().bold(),
                ENV_FILE.cyan(),
                ".env.dev.example".cyan(),
            );
            println!();
        }
    }

    // ── Resolve ports for health checks and ready banner ────────────────────
    let flux_port = port_of(&services, "flux").unwrap_or(DEFAULT_SERVER_PORT);
    let dash_port = port_of(&services, "dashboard").unwrap_or(DEFAULT_DASHBOARD_PORT);

    // ── Launch docker compose (async, non-blocking) ──────────────────────────
    //
    // Uses tokio::process::Command so the child process is tracked by the
    // tokio runtime and can be killed when Ctrl+C fires.
    let mut args = compose_args(&compose_path);
    args.push("up".to_string());
    args.push("--remove-orphans".to_string());

    let mut child = Command::new("docker")
        .args(&args)
        .env("FLUX_LOCAL", "true")
        // Inherit stdio so combined compose logs stream to the terminal.
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("'docker' not found \u{2014} install Docker Desktop first")
            } else {
                anyhow::anyhow!("Failed to start docker compose: {}", e)
            }
        })?;

    // Give the server a moment to initialise before health-checking.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Health-check the single monolithic server.
    let flux_url = format!("http://localhost:{}", flux_port);
    let flux_ok = wait_healthy(&flux_url, "/health", 60).await;

    println!();
    if flux_ok {
        println!("{}", "\u{2714}  Flux is running.".green().bold());
    } else {
        println!(
            "{}", "\u{26a0}  Server is still starting — check logs above.".yellow().bold()
        );
    }
    println!();
    println!("   {}  http://localhost:{}",           "Flux ".bold(), flux_port.to_string().cyan());
    println!("   {}  http://localhost:{}/flux/api",   "API  ".bold(), flux_port.to_string().cyan());
    println!("   {}  http://localhost:{}/flux",       "Dash ".bold(), dash_port.to_string().cyan());
    println!();
    println!("   {}  \u{2014} call a function",    "flux invoke <fn>   ".cyan().bold());
    println!("   {}  \u{2014} deploy functions",   "flux deploy        ".cyan().bold());
    println!("   {}  \u{2014} inspect a request",  "flux trace <id>    ".cyan().bold());
    println!("   {}  \u{2014} root-cause an error","flux why <id>      ".cyan().bold());
    println!();
    println!("{}", "Press Ctrl+C to stop.".dimmed());
    println!();

    // ── Wait for compose to exit or Ctrl+C ────────────────────────────────────
    //
    // `tokio::select!` races the child process against the interrupt signal.
    // On Ctrl+C we send SIGTERM to the child process group so all containers
    // are stopped cleanly (docker compose handles its own cleanup).
    tokio::select! {
        result = child.wait() => {
            let status = result?;
            if !status.success() {
                // Non-zero exit (e.g. compose failed to start a service).
                let code = status.code().unwrap_or(-1);
                anyhow::bail!("docker compose exited with status {}", code);
            }
        }
        _ = signal::ctrl_c() => {
            println!();
            println!("{}", "Stopping…".dimmed());
            // Kill the docker compose child; the compose process will in turn
            // stop all containers it manages.
            let _ = child.kill().await;
            let _ = child.wait().await;
            println!("{}", "\u{2714}  Stack stopped.".green());
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn port_of(services: &[ServiceInfo], name: &str) -> Option<u16> {
    services.iter().find(|s| s.name == name).map(|s| s.port)
}

