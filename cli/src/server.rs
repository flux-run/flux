//! `flux server` — run the full Fluxbase stack natively (no Docker required).
//!
//! Spawns all five backend services as child processes, multiplexes their
//! stdout/stderr into a single terminal with coloured service-name prefixes,
//! and shuts everything down cleanly on Ctrl+C.
//!
//! ```text
//! $ flux server                       # start all services (default ports)
//! $ flux server --port 9000           # shift base port (api=9000, gw=9001 …)
//! $ flux server --only api,gateway    # start a subset of services
//! $ flux server --release             # use release binaries
//! ```
//!
//! Binary resolution order (first match wins):
//!   1. Same directory as the `flux` binary            (self-host distribution)
//!   2. <workspace>/target/debug/<name>               (development build)
//!   3. <workspace>/target/release/<name>             (release build)
//!   4. System PATH

use std::path::PathBuf;
use std::sync::Arc;

use colored::Colorize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

// ─── Service descriptors ─────────────────────────────────────────────────────

#[derive(Clone)]
struct ServiceDef {
    /// Binary name (also the process name)
    name:  &'static str,
    /// Short label padded for aligned output
    label: &'static str,
    /// Default port offset from base (base defaults to 8080)
    port_offset: u16,
    /// Environment variable used to set the listening port
    port_env: &'static str,
    /// Additional env vars this service needs beyond the shared set
    extra_env: &'static [(&'static str, &'static str)],
    /// ANSI colour function applied to the label prefix
    colour: fn(&str) -> colored::ColoredString,
}

const SERVICES: &[ServiceDef] = &[
    ServiceDef {
        name:  "api",
        label: "api      ",
        port_offset: 0,
        port_env: "PORT",
        extra_env: &[],
        colour: |s| s.blue().bold(),
    },
    ServiceDef {
        name:  "gateway",
        label: "gateway  ",
        port_offset: 1,
        port_env: "GATEWAY_PORT",
        extra_env: &[],
        colour: |s| s.green().bold(),
    },
    ServiceDef {
        name:  "data-engine",
        label: "data-eng ",
        port_offset: 2,
        port_env: "PORT",
        extra_env: &[],
        colour: |s| s.cyan().bold(),
    },
    ServiceDef {
        name:  "runtime",
        label: "runtime  ",
        port_offset: 3,
        port_env: "PORT",
        extra_env: &[],
        colour: |s| s.magenta().bold(),
    },
    ServiceDef {
        name:  "queue",
        label: "queue    ",
        port_offset: 4,
        port_env: "QUEUE_PORT",
        extra_env: &[],
        colour: |s| s.yellow().bold(),
    },
];

// ─── Binary resolution ───────────────────────────────────────────────────────

/// Locate a service binary. Returns the path if found, None otherwise.
fn find_binary(name: &str, prefer_release: bool) -> Option<PathBuf> {
    let bin = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    // 1. Alongside the flux binary (self-host distribution)
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(&exe).join(&bin);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2 + 3. Workspace target directory (walk up to find Cargo.toml workspace)
    let workspace = find_workspace_root();
    if let Some(root) = workspace {
        let build_dirs: &[&str] = if prefer_release {
            &["target/release", "target/debug"]
        } else {
            &["target/debug", "target/release"]
        };
        for dir in build_dirs {
            let candidate = root.join(dir).join(&bin);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 4. System PATH
    which::which(&bin).ok()
}

/// Walk upward from cwd looking for a Cargo.toml with `[workspace]`.
fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml = dir.join("Cargo.toml");
        if toml.exists() {
            if let Ok(contents) = std::fs::read_to_string(&toml) {
                if contents.contains("[workspace]") {
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ─── Shared inter-service URL helpers ────────────────────────────────────────

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

// ─── Main entry point ────────────────────────────────────────────────────────

pub async fn execute(
    base_port:       u16,
    only:            Option<Vec<String>>,
    prefer_release:  bool,
    no_color:        bool,
    db_url_override: Option<String>,
) -> anyhow::Result<()> {
    if no_color {
        colored::control::set_override(false);
    }

    // Require DATABASE_URL (flag overrides env)
    let database_url = db_url_override
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| {
        eprintln!(
            "{} {} is not set.\n  {}",
            "✗".red().bold(),
            "DATABASE_URL".cyan(),
            "export DATABASE_URL=postgres://user:pass@localhost:5432/fluxbase".dimmed()
        );
        std::process::exit(1);
    });

    let token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "flux_dev_token".to_string());

    // Filter to requested services
    let active: Vec<&ServiceDef> = SERVICES
        .iter()
        .filter(|s| {
            only.as_ref().map_or(true, |list| {
                list.iter().any(|n| n.eq_ignore_ascii_case(s.name))
            })
        })
        .collect();

    if active.is_empty() {
        anyhow::bail!("No matching services found. Valid names: api, gateway, data-engine, runtime, queue");
    }

    // Resolve binary paths upfront — fail fast before spawning anything
    let mut resolved: Vec<(&ServiceDef, PathBuf)> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for svc in &active {
        match find_binary(svc.name, prefer_release) {
            Some(path) => resolved.push((svc, path)),
            None       => missing.push(svc.name),
        }
    }

    if !missing.is_empty() {
        eprintln!(
            "{} Could not find binaries for: {}",
            "✗".red().bold(),
            missing.join(", ").cyan()
        );
        eprintln!(
            "  Build them first: {}",
            "cargo build -p api -p gateway -p data-engine -p runtime -p queue".dimmed()
        );
        eprintln!(
            "  Or use Docker:    {}",
            "flux stack up".dimmed()
        );
        std::process::exit(1);
    }

    // ── Print startup banner ─────────────────────────────────────────────────
    println!();
    println!("  {} {}", "flux server".bold(), env!("CARGO_PKG_VERSION").dimmed());
    println!();
    for svc in &resolved {
        let port = base_port + svc.0.port_offset;
        println!(
            "  {}  {}",
            (svc.0.colour)(svc.0.label),
            format!("http://localhost:{port}").dimmed()
        );
    }
    println!();
    println!("  {}  {}", "database ".dimmed(), database_url.dimmed());
    println!();
    println!("  {}", "Press Ctrl+C to stop all services.".dimmed());
    println!();

    // ── Spawn all services ───────────────────────────────────────────────────
    let api_port      = base_port;
    let gateway_port  = base_port + 1;
    let de_port       = base_port + 2;
    let runtime_port  = base_port + 3;
    let queue_port    = base_port + 4;

    let shared_env: Vec<(String, String)> = vec![
        ("DATABASE_URL".into(),           database_url.clone()),
        ("INTERNAL_SERVICE_TOKEN".into(), token.clone()),
        ("RUST_LOG".into(),               std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())),
    ];

    // Service-specific env var sets
    let service_envs: std::collections::HashMap<&str, Vec<(String, String)>> = [
        ("api", vec![
            ("PORT".into(),             api_port.to_string()),
            ("DATA_ENGINE_URL".into(),  base_url(de_port)),
            ("GATEWAY_URL".into(),      base_url(gateway_port)),
        ]),
        ("gateway", vec![
            ("GATEWAY_PORT".into(),     gateway_port.to_string()),
            ("RUNTIME_URL".into(),      base_url(runtime_port)),
            ("DATA_ENGINE_URL".into(),  base_url(de_port)),
            ("API_URL".into(),          base_url(api_port)),
            ("QUEUE_URL".into(),        base_url(queue_port)),
        ]),
        ("data-engine", vec![
            ("PORT".into(),             de_port.to_string()),
            ("RUNTIME_URL".into(),      base_url(runtime_port)),
        ]),
        ("runtime", vec![
            ("PORT".into(),             runtime_port.to_string()),
            ("CONTROL_PLANE_URL".into(),base_url(api_port)),
            ("SERVICE_TOKEN".into(),    token.clone()),
        ]),
        ("queue", vec![
            ("QUEUE_PORT".into(),       queue_port.to_string()),
            ("RUNTIME_URL".into(),      base_url(runtime_port)),
            ("WORKER_CONCURRENCY".into(),"10".into()),
            ("WORKER_POLL_INTERVAL_MS".into(), "500".into()),
        ]),
    ].into_iter().collect();

    // Track all child PIDs so we can kill them on Ctrl+C
    let children: Arc<Mutex<Vec<tokio::process::Child>>> = Arc::new(Mutex::new(Vec::new()));
    let children_ctrlc = children.clone();

    let mut tasks = tokio::task::JoinSet::new();

    for (svc, bin_path) in resolved {
        let mut cmd = Command::new(&bin_path);

        // Apply shared env
        for (k, v) in &shared_env {
            cmd.env(k, v);
        }
        // Apply service-specific env
        if let Some(extras) = service_envs.get(svc.name) {
            for (k, v) in extras {
                cmd.env(k, v);
            }
        }
        // Allow caller's environment to override (DATABASE_URL from shell, etc.)
        cmd.env("DATABASE_URL", &database_url);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!("Failed to spawn {}: {}", svc.name, e)
        })?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        children.lock().await.push(child);

        // Stream stdout
        let label   = svc.label;
        let colour  = svc.colour;
        tasks.spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                println!("[{}] {}", colour(label), line);
            }
        });

        // Stream stderr
        let label2  = svc.label;
        let colour2 = svc.colour;
        tasks.spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[{}] {}", colour2(label2), line);
            }
        });
    }

    // ── Ctrl+C handler ───────────────────────────────────────────────────────
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!();
        println!("{} Shutting down services…", "■".red().bold());
        let mut locked = children_ctrlc.lock().await;
        for child in locked.iter_mut() {
            let _ = child.kill().await;
        }
        std::process::exit(0);
    });

    // Wait for all log-streaming tasks (they end when the child processes exit)
    while tasks.join_next().await.is_some() {}

    Ok(())
}
