//! `flux serve` / `flux server` — start the Flux monolithic server.
//!
//! All services (gateway, runtime, api, data-engine, queue) are embedded in a
//! single `server` binary and communicate in-process — no HTTP between them.
//!
//! ```text
//! $ flux serve                    # port 8080, debug binary
//! $ flux serve --port 3000        # custom port
//! $ flux serve --release          # release binary
//! ```
//!
//! Binary resolution (first match wins):
//!   1. Same directory as the `flux` binary  (self-host distribution)
//!   2. <workspace>/target/debug/server      (dev build)
//!   3. <workspace>/target/release/server    (release build)
//!   4. System PATH

use std::path::PathBuf;

use colored::Colorize;

fn find_server_binary(prefer_release: bool) -> Option<PathBuf> {
    let bin = if cfg!(windows) { "server.exe" } else { "server" };

    // 1. Alongside the flux binary (self-host distribution)
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(&exe).join(bin);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2 + 3. Workspace target directory
    if let Some(root) = find_workspace_root() {
        let dirs: &[&str] = if prefer_release {
            &["target/release", "target/debug"]
        } else {
            &["target/debug", "target/release"]
        };
        for dir in dirs {
            let candidate = root.join(dir).join(bin);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 4. System PATH
    which::which(bin).ok()
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml = dir.join("Cargo.toml");
        if toml.exists() {
            if let Ok(c) = std::fs::read_to_string(&toml) {
                if c.contains("[workspace]") {
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub async fn execute(
    port:            u16,
    prefer_release:  bool,
    no_color:        bool,
    db_url_override: Option<String>,
) -> anyhow::Result<()> {
    if no_color {
        colored::control::set_override(false);
    }

    let database_url = db_url_override
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| {
            eprintln!(
                "{} {} is not set.\n  {}",
                "✗".red().bold(),
                "DATABASE_URL".cyan(),
                "export DATABASE_URL=postgres://user:pass@localhost:5432/fluxbase".dimmed(),
            );
            std::process::exit(1);
        });

    let bin = find_server_binary(prefer_release).unwrap_or_else(|| {
        eprintln!(
            "{} Could not find the {} binary.",
            "✗".red().bold(),
            "server".cyan(),
        );
        eprintln!(
            "  Build it first: {}",
            "cargo build -p server".dimmed(),
        );
        std::process::exit(1);
    });

    println!();
    println!("  {} {}", "flux server".bold(), env!("CARGO_PKG_VERSION").dimmed());
    println!("  {}  {}", "address  ".dimmed(), format!("http://localhost:{port}").cyan());
    println!("  {}  {}", "database ".dimmed(), database_url.dimmed());
    println!("  {}  {}", "binary   ".dimmed(), bin.display().to_string().dimmed());
    println!();

    let token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "flux_dev_token".to_string());

    // Replace the current process with the server — signals propagate correctly.
    // On Windows this falls back to a child-process spawn.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&bin)
            .env("PORT",                   port.to_string())
            .env("DATABASE_URL",           &database_url)
            .env("INTERNAL_SERVICE_TOKEN", &token)
            .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
            .exec();
        return Err(anyhow::anyhow!("Failed to exec server binary: {err}"));
    }

    #[cfg(not(unix))]
    {
        let status = tokio::process::Command::new(&bin)
            .env("PORT",                   port.to_string())
            .env("DATABASE_URL",           &database_url)
            .env("INTERNAL_SERVICE_TOKEN", &token)
            .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
            .status()
            .await?;

        if !status.success() {
            anyhow::bail!("server exited with {status}");
        }
        Ok(())
    }
}
