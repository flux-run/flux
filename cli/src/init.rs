//! `flux init [name]` — bootstrap a complete Flux project from the base scaffold.
//!
//! ```text
//! $ flux init my-app
//! ◆  Initialising my-app
//!
//!   ✔  flux.toml
//!   ✔  gateway.toml
//!   ✔  .gitignore
//!   ✔  .env.example
//!   ...
//!
//!   Next steps:
//!     1.  cd my-app
//!     2.  flux dev
//! ```
//!
//! Scaffold files are embedded at compile time via `include_str!()` from
//! `scaffolds/base/`.  The only substitution token is `{name}` (project name).

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use colored::Colorize;

// ── Base scaffold (embedded at compile time) ──────────────────────────────────
// Each tuple is (relative output path, content with {name} placeholder).

fn base_scaffold_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("flux.toml",                          include_str!("../../scaffolds/base/flux.toml")),
        ("gateway.toml",                       include_str!("../../scaffolds/base/gateway.toml")),
        (".gitignore",                         include_str!("../../scaffolds/base/.gitignore")),
        (".env.example",                       include_str!("../../scaffolds/base/.env.example")),
        ("README.md",                          include_str!("../../scaffolds/base/README.md")),
        ("functions/hello/index.ts",           include_str!("../../scaffolds/base/functions/hello/index.ts")),
        ("functions/hello/flux.json",          include_str!("../../scaffolds/base/functions/hello/flux.json")),
        ("schemas/_types.ts",                  include_str!("../../scaffolds/base/schemas/_types.ts")),
        ("schemas/_shared/auth.ts",            include_str!("../../scaffolds/base/schemas/_shared/auth.ts")),
        ("schemas/_shared/jsonb.ts",           include_str!("../../scaffolds/base/schemas/_shared/jsonb.ts")),
        ("schemas/users.schema.ts",            include_str!("../../scaffolds/base/schemas/users.schema.ts")),
        ("middleware/auth.ts",                 include_str!("../../scaffolds/base/middleware/auth.ts")),
        ("queues/email.queue.toml",            include_str!("../../scaffolds/base/queues/email.queue.toml")),
    ]
}

// ── Option bag ────────────────────────────────────────────────────────────────

pub struct InitOptions {
    /// Project name. Defaults to current directory name.
    pub name:         Option<String>,
    /// Override local gateway port in `[dev]` section.
    pub gateway_port: Option<u16>,
}

pub async fn execute(opts: InitOptions) -> anyhow::Result<()> {
    // Resolve project name ─────────────────────────────────────────────────────
    let project_name = opts.name.as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "my-project".to_string())
        });

    // Determine target directory ───────────────────────────────────────────────
    // If a name was explicitly provided, create a new subdirectory.
    // Otherwise, initialise in the current directory.
    let target_dir: PathBuf = if opts.name.is_some() {
        std::env::current_dir()
            .unwrap_or_default()
            .join(&project_name)
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    if opts.name.is_some() && target_dir.exists() {
        anyhow::bail!(
            "Directory '{}' already exists. Choose a different name or remove it first.",
            project_name
        );
    }

    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("Failed to create directory {}", target_dir.display()))?;

    println!();
    println!(
        "{} Initialising {}",
        "◆".cyan().bold(),
        project_name.cyan().bold()
    );
    println!();

    // Write scaffold files ─────────────────────────────────────────────────────
    for (rel_path, content) in base_scaffold_files() {
        let substituted = content.replace("{name}", &project_name);
        write_file(&target_dir, rel_path, &substituted)?;
        println!("  {}  {}", "✔".green().bold(), rel_path.cyan());
    }

    // Write .flux/config.json ─────────────────────────────────────────────────
    let gw_port = opts.gateway_port.unwrap_or(4000);
    write_flux_config(&target_dir, gw_port)?;
    println!("  {}  {}", "✔".green().bold(), ".flux/config.json".cyan());

    println!();
    println!("  {}  {}", "name".bold(), project_name.cyan());
    println!();

    if opts.name.is_some() {
        println!("  {}", "Next steps:".bold());
        println!("    1.  cd {}", project_name.cyan());
        println!("    2.  {}", "flux dev".cyan());
    } else {
        println!("  {}", "Next steps:".bold());
        println!("    1.  {}", "flux dev".cyan());
    }
    println!();

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_file(base: &Path, rel: &str, content: &str) -> anyhow::Result<()> {
    let path = base.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn write_flux_config(base: &Path, gateway_port: u16) -> anyhow::Result<()> {
    let content = format!(
        r#"{{
  "server_url": "http://localhost:{gateway_port}/flux/api",
  "cli_key": null
}}
"#
    );
    let flux_dir = base.join(".flux");
    std::fs::create_dir_all(&flux_dir)
        .with_context(|| format!("Failed to create {}", flux_dir.display()))?;
    std::fs::write(flux_dir.join("config.json"), content)
        .with_context(|| "Failed to write .flux/config.json".to_string())
}
