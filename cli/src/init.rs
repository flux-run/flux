//! `flux init` — create `flux.toml` for this project directory.
//!
//! ```text
//! $ flux init
//! ✔  Created flux.toml
//!
//!    name    = "my-project"
//!    runtime = "nodejs20"
//!
//!    [record]  sample_rate = 1.0   retention_days = 30
//!    [limits]  timeout_ms = 5000   memory_mb = 256
//!    [dev]     gateway :4000  runtime :8083  api :8080  ...
//!
//!    Commit flux.toml to version control.
//!    Run: flux dev
//! ```

use std::fmt::Write as FmtWrite;

use colored::Colorize;
use tokio::fs;

// ─── Option bag ──────────────────────────────────────────────────────────────
//
// Grouped into a struct so the public API stays clean and adding fields later
// doesn't require changing every call site.

pub struct InitOptions {
    /// Project name written to `flux.toml`.  Defaults to cwd folder name.
    pub name:         Option<String>,
    /// Runtime identifier (e.g. `nodejs20`, `bun`, `deno`).
    pub runtime:      Option<String>,
    /// Override local API port in `[dev]` section.
    pub api_port:     Option<u16>,
    /// Override local gateway port in `[dev]` section.
    pub gateway_port: Option<u16>,
    /// Override local runtime port in `[dev]` section.
    pub runtime_port: Option<u16>,
}

pub async fn execute(opts: InitOptions) -> anyhow::Result<()> {
    // Resolve defaults ────────────────────────────────────────────────────────
    let project_name = opts.name.as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "my-project".to_string())
        });

    let runtime_str  = opts.runtime.as_deref().unwrap_or("nodejs20");
    let gw_port      = opts.gateway_port.unwrap_or(4000);
    let rt_port      = opts.runtime_port.unwrap_or(8083);
    let api_port_val = opts.api_port.unwrap_or(8080);

    // Build flux.toml ─────────────────────────────────────────────────────────
    //
    // Written manually with `write!` rather than `format!()` so that values
    // containing TOML special characters are inserted safely.  Project names
    // are double-quoted; quotes inside names are escaped.
    let toml_content = build_flux_toml(
        &project_name, runtime_str,
        gw_port, rt_port, api_port_val,
    );

    let flux_toml_path = std::path::Path::new("flux.toml");

    if flux_toml_path.exists() {
        println!(
            "{} {} already exists \u{2014} skipping (delete it first to regenerate)",
            "⚠".yellow().bold(),
            "flux.toml".cyan(),
        );
    } else {
        fs::write(flux_toml_path, &toml_content).await?;
        println!("{} Created {}", "✔".green().bold(), "flux.toml".cyan().bold());
    }

    // Echo key settings ───────────────────────────────────────────────────────
    println!();
    println!("  {:<10}  {}", "name".bold(),    project_name.cyan());
    println!("  {:<10}  {}", "runtime".bold(), runtime_str.cyan());
    println!();
    println!("  {:<10}  gateway :{}", "[dev]".bold(), gw_port.to_string().cyan());
    println!("  {:<10}  runtime :{}",  "".bold(),     rt_port.to_string().cyan());
    println!("  {:<10}  api     :{}",  "".bold(),     api_port_val.to_string().cyan());
    println!();
    println!("{}", "Commit flux.toml to version control.".dimmed());
    println!("Run: {}", "flux dev".cyan().bold());

    Ok(())
}

// ─── TOML builder ────────────────────────────────────────────────────────────
//
// Avoids `format!()` with user-supplied data in the format string.
// TOML strings double-quote the value; interior quotes are escaped.

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn build_flux_toml(
    name:         &str,
    runtime:      &str,
    gateway_port: u16,
    runtime_port: u16,
    api_port:     u16,
) -> String {
    let mut out = String::with_capacity(512);

    let _ = writeln!(out, "# Flux project configuration");
    let _ = writeln!(out, "# Commit this file to version control.");
    let _ = writeln!(out);
    let _ = writeln!(out, r#"name    = "{}""#, toml_escape(name));
    let _ = writeln!(out, r#"runtime = "{}""#, toml_escape(runtime));
    let _ = writeln!(out);
    let _ = writeln!(out, "[record]");
    let _ = writeln!(out, "# 1.0 = record every execution. Values below 1.0 mean some executions");
    let _ = writeln!(out, "# won't appear in `flux trace`. Only lower this above ~1k rps.");
    let _ = writeln!(out, "sample_rate    = 1.0");
    let _ = writeln!(out, "retention_days = 30");
    let _ = writeln!(out);
    let _ = writeln!(out, "[limits]");
    let _ = writeln!(out, "# Default per-function limits. Override in defineFunction() or flux.json.");
    let _ = writeln!(out, "timeout_ms = 5000");
    let _ = writeln!(out, "memory_mb  = 256");
    let _ = writeln!(out);
    let _ = writeln!(out, "[dev]");
    let _ = writeln!(out, "# Local port assignments used by `flux dev`. Adjust to avoid conflicts.");
    let _ = writeln!(out, "gateway_port     = {}", gateway_port);
    let _ = writeln!(out, "runtime_port     = {}", runtime_port);
    let _ = writeln!(out, "api_port         = {}", api_port);
    let _ = writeln!(out, "data_engine_port = 8082");
    let _ = writeln!(out, "queue_port       = 8084");

    out
}

