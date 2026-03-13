//! `flux db push` — apply user-owned SQL migrations to a Flux database.
//!
//! User migrations live in `schemas/` (configurable via `flux.toml`
//! `db.migrations_dir`).  They are tracked in `flux.user_migrations` —
//! completely separate from the Flux system migrations in
//! `schemas/api/` and `schemas/data-engine/`.
//!
//! ```text
//! $ flux db push
//! ◆ Applying migrations → local (http://localhost:4000)
//!
//!   ✔ 001_create_users.sql        already applied
//!   ↑ 002_add_email_index.sql     applying…  ✔ done
//!   ↑ 003_create_orders.sql       applying…  ✔ done
//!
//!   2 migration(s) applied.
//!
//! $ flux db push --context prod
//! ◆ Applying migrations → prod (https://myapp.com)
//! …
//! ```
//!
//! ## How it works
//!
//! 1. Scan `schemas/` and sort files lexicographically (conventional
//!    `NNN_name.sql` prefix guarantees order).
//! 2. POST each file's content to `POST /internal/db/migrate` on the connected
//!    Flux server.  The server is the single authority on what is applied —
//!    it checks `flux.user_migrations` and executes idempotently.
//!
//! ## SOLID
//!
//! - SRP: scanning + sorting, network I/O, and output are separate functions.
//! - DIP: transport is `reqwest` behind `apply_migration()`; replacing it with
//!   a direct DB connection requires changing only that function.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::context::{ResolvedContext, resolve_context};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MigrateRequest {
    name:    String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MigrateResponse {
    status:  String, // "applied" | "already_applied"
    message: Option<String>,
}

/// Schema manifest emitted by the Deno compiler (one per *.schema.ts file).
#[derive(Debug, Serialize, Deserialize)]
struct SchemaManifest {
    table:        String,
    file:         String,
    #[serde(default)]
    columns:      serde_json::Value,
    #[serde(default)]
    indexes:      serde_json::Value,
    #[serde(default)]
    foreign_keys: serde_json::Value,
    #[serde(default)]
    rules:        serde_json::Value,
    #[serde(default)]
    hooks:        serde_json::Value,
    #[serde(default)]
    on:           serde_json::Value,
}

// Embedded Deno compiler script (compiled into the binary at build time).
const SCHEMA_COMPILER: &str = include_str!("../scripts/flux-schema-compiler.ts");

// ── Entry point ───────────────────────────────────────────────────────────────

/// Execute `flux db push [--context <name>] [--dir <dir>]`.
pub async fn execute_db_push(
    context_name: Option<String>,
    migrations_dir: Option<String>,
) -> anyhow::Result<()> {
    let project_root = crate::dev::find_project_root_pub();
    let ctx = resolve_context(context_name.as_deref(), project_root.as_deref())?;

    // Resolve migrations directory: flag > flux.toml db.migrations_dir > "schemas"
    let dir = resolve_migrations_dir(migrations_dir.as_deref(), project_root.as_deref())?;

    println!();
    println!(
        "{} Applying migrations  {} {}",
        "◆".cyan().bold(),
        "→".dimmed(),
        format!("{} ({})", ctx.name.cyan().bold(), ctx.endpoint.dimmed())
    );
    println!();

    let files = collect_migrations(&dir)?;
    if files.is_empty() {
        println!("  {}", "No migration files found.".yellow());
        println!("  Add .sql files to: {}", dir.display().to_string().cyan());
        println!();
        return Ok(());
    }

    let client = reqwest::Client::new();
    let mut applied = 0usize;

    for file in &files {
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let content = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;

        // Print status line before the request.
        print!("  {} {:<50}", "↑".blue(), name.cyan());
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let resp = send_migration(&client, &ctx, &name, &content).await;

        match resp {
            Ok(r) if r.status == "already_applied" => {
                // Overwrite the line.
                println!("\r  {} {:<50}", "✔".green(), name.dimmed());
            }
            Ok(r) if r.status == "applied" => {
                println!("\r  {} {:<50} {}", "✔".green().bold(), name.cyan(), "applied".green());
                applied += 1;
            }
            Ok(r) => {
                println!("\r  {} {:<50} ({})", "?".yellow(), name, r.status.yellow());
            }
            Err(e) => {
                println!("\r  {} {:<50}", "✗".red().bold(), name.red());
                eprintln!("     {}", e.to_string().red());
                bail!("Migration failed: {}", name);
            }
        }
    }

    println!();
    if applied == 0 {
        println!("  {} All migrations already applied.", "✔".green().bold());
    } else {
        println!(
            "  {} {} migration(s) applied.",
            "✔".green().bold(),
            applied.to_string().cyan().bold()
        );
    }
    println!();

    // ── Schema compilation (*.schema.ts → RuleExpr/TransformExpr AST) ────────
    push_schemas(&ctx, project_root.as_deref()).await?;

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect `.sql` files from `dir`, sorted lexicographically.
fn collect_migrations(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read migrations dir: {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "sql").unwrap_or(false))
        .collect();

    files.sort();
    Ok(files)
}

/// Determine migrations directory from: explicit flag > flux.toml > default.
fn resolve_migrations_dir(
    flag: Option<&str>,
    project_root: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    // 1. Explicit flag
    if let Some(d) = flag {
        return Ok(PathBuf::from(d));
    }

    // 2. flux.toml db.migrations_dir
    let root = project_root.unwrap_or_else(|| Path::new("."));
    let toml_path = root.join("flux.toml");
    if toml_path.exists() {
        let raw = std::fs::read_to_string(&toml_path)?;
        if let Ok(value) = raw.parse::<toml::Value>() {
            if let Some(dir) = value
                .get("db")
                .and_then(|d| d.get("migrations_dir"))
                .and_then(|v| v.as_str())
            {
                return Ok(root.join(dir));
            }
        }
    }

    // 3. Default
    Ok(root.join("schemas"))
}

/// POST a single migration to the Flux server.
async fn send_migration(
    client: &reqwest::Client,
    ctx: &ResolvedContext,
    name: &str,
    content: &str,
) -> anyhow::Result<MigrateResponse> {
    let url = format!("{}/internal/db/migrate", ctx.endpoint);
    let body = MigrateRequest {
        name:    name.to_owned(),
        content: content.to_owned(),
    };

    let mut req = client.post(&url).json(&body);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    } else {
        req = req.header("x-service-token", "dev-token-local");
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("Failed to connect to {}", url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Server returned {}: {}", status, body);
    }

    let result: MigrateResponse = resp.json().await.context("Invalid response from server")?;
    Ok(result)
}

// ── Schema compilation pipeline ───────────────────────────────────────────────

/// Run `flux-schema-compiler.ts` via Deno to compile all `*.schema.ts` files,
/// then POST each manifest to the server's `/internal/db/schema` endpoint.
async fn push_schemas(
    ctx: &ResolvedContext,
    project_root: Option<&Path>,
) -> anyhow::Result<()> {
    let root = project_root.unwrap_or_else(|| Path::new("."));
    let schemas_dir = root.join("schemas");

    if !schemas_dir.exists() {
        return Ok(());
    }

    // Check for any *.schema.ts files
    let has_schemas = std::fs::read_dir(&schemas_dir)
        .ok()
        .map(|entries| entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".schema.ts")))
        .unwrap_or(false);

    if !has_schemas {
        return Ok(());
    }

    // Find Deno binary
    let deno = crate::toolchain::toolchain_path("typescript")
        .ok_or_else(|| anyhow::anyhow!(
            "Deno not found. Run: flux toolchain install typescript"
        ))?;

    // Write compiler script and import map to temp dir
    let tmp_dir = std::env::temp_dir();
    let compiler_path = tmp_dir.join("flux-schema-compiler.ts");
    std::fs::write(&compiler_path, SCHEMA_COMPILER)
        .context("Failed to write schema compiler script")?;

    // Build an import map that resolves @fluxbase/schema to the local package.
    // Look for packages/schema/src/index.ts relative to the project root.
    let schema_pkg = root.join("packages").join("schema").join("src").join("runtime.ts");
    let import_map_path = tmp_dir.join("flux-schema-compiler-importmap.json");
    let import_map = if schema_pkg.exists() {
        let schema_url = format!("file://{}", schema_pkg.display());
        serde_json::json!({
            "imports": {
                "@fluxbase/schema": schema_url,
            }
        })
    } else {
        serde_json::json!({ "imports": {} })
    };
    std::fs::write(&import_map_path, serde_json::to_string_pretty(&import_map)?)
        .context("Failed to write import map")?;

    println!(
        "{} Compiling schemas  {} {}",
        "◆".cyan().bold(),
        "→".dimmed(),
        format!("{} ({})", ctx.name.cyan().bold(), ctx.endpoint.dimmed())
    );
    println!();

    // Run: deno run --allow-read --allow-env --unstable-sloppy-imports --import-map=<map> <compiler> <schemas_dir>
    let output = std::process::Command::new(&deno)
        .args([
            "run",
            "--allow-read",
            "--allow-env",
            "--unstable-sloppy-imports",
            &format!("--import-map={}", import_map_path.display()),
            compiler_path.to_str().unwrap_or(""),
            schemas_dir.to_str().unwrap_or("schemas"),
        ])
        .current_dir(root)
        .output()
        .with_context(|| format!("Failed to run Deno compiler: {}", deno.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Schema compiler failed:\n{}", stderr);
    }

    // Parse NDJSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let client = reqwest::Client::new();
    let mut pushed = 0usize;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }

        // Check for compiler errors on stderr lines interleaved as JSON
        let val: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("Invalid compiler output: {line}"))?;

        if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
            bail!("Schema compiler error: {err}");
        }

        let manifest: SchemaManifest = serde_json::from_value(val)
            .with_context(|| "Failed to parse schema manifest")?;

        print!("  {} {:<50}", "↑".blue(), manifest.table.cyan());
        use std::io::Write;
        let _ = std::io::stdout().flush();

        match send_schema(&client, ctx, &manifest).await {
            Ok(status) if status == "already_applied" => {
                println!("\r  {} {:<50}", "✔".green(), manifest.table.dimmed());
            }
            Ok(_) => {
                println!("\r  {} {:<50} {}", "✔".green().bold(), manifest.table.cyan(), "pushed".green());
                pushed += 1;
            }
            Err(e) => {
                println!("\r  {} {:<50}", "✗".red().bold(), manifest.table.red());
                eprintln!("     {}", e.to_string().red());
                bail!("Schema push failed for table: {}", manifest.table);
            }
        }
    }

    println!();
    if pushed == 0 {
        println!("  {} All schemas up to date.", "✔".green().bold());
    } else {
        println!(
            "  {} {} schema(s) pushed.",
            "✔".green().bold(),
            pushed.to_string().cyan().bold()
        );
    }
    println!();

    std::fs::remove_file(&compiler_path).ok();
    std::fs::remove_file(&import_map_path).ok();
    Ok(())
}

/// POST a compiled schema manifest to the server.
async fn send_schema(
    client: &reqwest::Client,
    ctx: &ResolvedContext,
    manifest: &SchemaManifest,
) -> anyhow::Result<String> {
    let url = format!("{}/internal/db/schema", ctx.endpoint);

    let mut req = client.post(&url).json(manifest);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    } else {
        req = req.header("x-service-token", "dev-token-local");
    }

    let resp = req.send().await
        .with_context(|| format!("Failed to connect to {url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Server returned {}: {}", status, body);
    }

    // Server returns {"status": "applied" | "already_applied"}
    let body: serde_json::Value = resp.json().await.context("Invalid response from server")?;
    Ok(body["status"].as_str().unwrap_or("applied").to_owned())
}
