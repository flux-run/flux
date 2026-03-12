//! `flux dev` — start the full Flux development stack locally.
//!
//! ```text
//! $ flux dev
//!
//! ▶  Starting Flux dev stack …
//!
//!    db            postgres     :5432
//!    api           management   :8080
//!    gateway       execution    :8081   LOCAL_MODE=true
//!    data-engine   query        :8082
//!    runtime       functions    :8083
//!    queue         workers      :8084
//!    dashboard     spa          :5173
//!
//! ✔  All services healthy — Flux is running.
//!
//!    API      http://localhost:8080
//!    Gateway  http://localhost:8081
//!    Dash     http://localhost:5173
//!
//!    flux invoke <fn>        — call a function
//!    flux deploy             — deploy changed functions
//!    flux trace <id>         — inspect a request
//!    flux why <id>           — root-cause an error
//!
//!    Press Ctrl+C to stop.
//! ```
//!
//! Wraps `docker compose -f docker-compose.dev.yml up` with `FLUX_LOCAL=true`
//! so the gateway skips tenant auth in local dev.
//! Port overrides are read from `flux.toml [dev]`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;
use tokio::process::Command;
use tokio::signal;

use crate::config::FluxToml;

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
        ServiceInfo { name: "db",          label: "postgres",   port: 5432, local_mode: false },
        ServiceInfo { name: "api",         label: "management", port: 8080, local_mode: false },
        ServiceInfo { name: "gateway",     label: "execution",  port: 8081, local_mode: true  },
        ServiceInfo { name: "data-engine", label: "query",      port: 8082, local_mode: false },
        ServiceInfo { name: "runtime",     label: "functions",  port: 8083, local_mode: false },
        ServiceInfo { name: "queue",       label: "workers",    port: 8084, local_mode: false },
        ServiceInfo { name: "dashboard",   label: "spa",        port: 5173, local_mode: false },
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
                "gateway"     => t.dev.gateway_port,
                "runtime"     => t.dev.runtime_port,
                "api"         => t.dev.api_port,
                "data-engine" => t.dev.data_engine_port,
                "queue"       => t.dev.queue_port,
                _             => None,
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

    // ── Resolve ports for healthchecks and ready banner ──────────────────────
    let gateway_port = port_of(&services, "gateway").unwrap_or(8081);
    let api_port     = port_of(&services, "api").unwrap_or(8080);
    let dash_port    = port_of(&services, "dashboard").unwrap_or(5173);

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

    // Give containers a moment to initialise before health-checking.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Run health checks in the background — print the ready banner as soon as
    // both gateway and API respond, without blocking compose log output.
    let gw_url  = format!("http://localhost:{}", gateway_port);
    let api_url = format!("http://localhost:{}", api_port);

    let (gw_ok, api_ok) = tokio::join!(
        wait_healthy(&gw_url,  "/health", 45),
        wait_healthy(&api_url, "/health", 45),
    );

    println!();
    if gw_ok && api_ok {
        println!("{}", "\u{2714}  All services healthy \u{2014} Flux is running.".green().bold());
    } else {
        println!(
            "{}", "\u{26a0}  Some services are still starting \u{2014} check logs above.".yellow().bold()
        );
    }
    println!();
    println!("   {}  http://localhost:{}", "API    ".bold(), api_port.to_string().cyan());
    println!("   {}  http://localhost:{}", "Gateway".bold(), gateway_port.to_string().cyan());
    println!("   {}  http://localhost:{}", "Dash   ".bold(), dash_port.to_string().cyan());
    println!();
    println!("   {}  \u{2014} call a function",     "flux invoke <fn>   ".cyan().bold());
    println!("   {}  \u{2014} deploy functions",    "flux deploy        ".cyan().bold());
    println!("   {}  \u{2014} inspect a request",   "flux trace <id>    ".cyan().bold());
    println!("   {}  \u{2014} root-cause an error", "flux why <id>      ".cyan().bold());
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

