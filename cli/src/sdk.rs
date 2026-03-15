//! `flux pull` and `flux watch` — SDK code generation commands.
//!
//! # Pull
//!
//! Fetches the live schema from your Flux project and writes a fully-typed
//! TypeScript SDK file:
//!
//! ```text
//! $ flux pull
//! ✔ Connected to project  proj_abc123
//! ✔ Schema  v4  (hash: a3f8c1d2)
//! ✔ Tables  8   Functions  3
//! ✔ SDK written → src/flux.generated.ts
//! ```
//!
//! # Watch
//!
//! Polls the schema hash every N seconds (default 5) and regenerates the SDK
//! file whenever the schema changes:
//!
//! ```text
//! $ flux watch --interval 10
//! ◉ Watching schema (every 10s) → flux.generated.ts
//!   [10:02:31] ✔ v4  unchanged
//!   [10:02:41] ↻ v5  schema changed – regenerating…
//!   [10:02:41] ✔ SDK written → flux.generated.ts
//! ```

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use colored::Colorize;
use serde::Deserialize;
use tokio::{fs, time};

use crate::client::ApiClient;
use crate::config::ProjectConfig;

// ─── Response shapes ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SchemaResponse {
    schema_hash:    String,
    schema_version: Option<i64>,
    tables:         Option<Vec<serde_json::Value>>,
    functions:      Option<Vec<serde_json::Value>>,
}

// ─── API helpers ──────────────────────────────────────────────────────────────

async fn fetch_schema(client: &ApiClient) -> anyhow::Result<SchemaResponse> {
    let url = format!("{}/sdk/schema", client.base_url);
    let res = client.client.get(&url).send().await
        .context("Failed to reach Flux API")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("API returned {status}: {body}");
    }

    // The API wraps responses in { data: { ... } }
    let envelope: serde_json::Value = res.json().await
        .context("Failed to parse schema response")?;

    let inner = envelope.get("data").cloned().unwrap_or(envelope);
    serde_json::from_value(inner).context("Unexpected schema response shape")
}

async fn fetch_typescript_sdk(client: &ApiClient) -> anyhow::Result<String> {
    let url = format!("{}/sdk/typescript", client.base_url);
    let res = client.client.get(&url).send().await
        .context("Failed to reach Flux API")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("API returned {status}: {body}");
    }

    res.text().await.context("Failed to read SDK response body")
}

// ─── Metadata banner ─────────────────────────────────────────────────────────

/// Prepend a structured comment header to the generated SDK source.
/// This makes it trivial to inspect which schema version is baked in,
/// and `flux status` can parse it back out without a network call.
fn prepend_header(
    sdk_source: &str,
    project_id: &str,
    version: i64,
    hash: &str,
) -> String {
    let ts = iso_now();
    format!(
        "/**\n\
         * @generated Flux SDK\n\
         * Project:        {project_id}\n\
         * Schema version: v{version}\n\
         * Schema hash:    {hash}\n\
         * Generated:      {ts}\n\
         *\n\
         * DO NOT EDIT — regenerate with: flux pull\n\
         */\n\n{sdk_source}"
    )
}

/// Parse the schema version embedded in a previously generated SDK file.
/// Returns `None` if the file doesn't contain a recognized header.
pub fn parse_local_version(source: &str) -> Option<(i64, String)> {
    let mut version = None;
    let mut hash    = None;
    for line in source.lines().take(15) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("* Schema version: v") {
            version = rest.trim().parse::<i64>().ok();
        }
        if let Some(rest) = line.strip_prefix("* Schema hash:    ") {
            hash = Some(rest.trim().to_string());
        }
    }
    version.zip(hash)
}

// ─── Write helper ─────────────────────────────────────────────────────────────

async fn write_sdk(sdk_source: &str, output_path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("Could not create directory {:?}", parent))?;
        }
    }
    fs::write(output_path, sdk_source).await
        .with_context(|| format!("Failed to write SDK to {:?}", output_path))
}

// ─── Formatting helpers ────────────────────────────────────────────────────────

fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(8)]
}

/// Return current UTC time as an ISO 8601 string (seconds precision).
fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Decompose epoch seconds into Y-M-D H:M:S (no external crate needed).
    let (y, mo, d, h, mi, s) = epoch_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn now_hms() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (_, _, _, h, mi, s) = epoch_to_ymd_hms(secs);
    format!("{h:02}:{mi:02}:{s:02}")
}

/// Minimal epoch → (year, month, day, hour, min, sec) converter.
fn epoch_to_ymd_hms(mut t: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s  = t % 60; t /= 60;
    let mi = t % 60; t /= 60;
    let h  = t % 24; t /= 24;
    // Days since 1970-01-01
    let mut days = t;
    let mut y = 1970u64;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year { break; }
        days -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u64;
    for &md in &month_days {
        if days < md { break; }
        days -= md;
        mo += 1;
    }
    (y, mo, days + 1, h, mi, s)
}

// ─── flux pull ────────────────────────────────────────────────────────────────

/// Execute `flux pull`.
///
/// `output` — destination file path (defaults to `flux.generated.ts`).
pub async fn execute_pull(output: Option<String>) -> anyhow::Result<()> {
    let proj = ProjectConfig::load().await;
    let output_path = PathBuf::from(ProjectConfig::resolve_sdk_output(output, proj.as_ref()));

    let client = ApiClient::new().await?;

    // Print the project context so the user sees what they're pulling from.
    let project_id = "(use flux generate for types)";
    println!(
        "{} Connected to project  {}",
        "✔".green().bold(),
        project_id.cyan()
    );

    // Fetch schema metadata first (cheap call).
    print!("{} Fetching schema… ", "◆".blue());
    let schema = fetch_schema(&client).await?;
    let version   = schema.schema_version.unwrap_or(0);
    let n_tables  = schema.tables.as_ref().map(|t| t.len()).unwrap_or(0);
    let n_funcs   = schema.functions.as_ref().map(|f| f.len()).unwrap_or(0);
    let hash_disp = short_hash(&schema.schema_hash);

    println!(
        "\r{} Schema  {}  (hash: {})",
        "✔".green().bold(),
        format!("v{version}").yellow().bold(),
        hash_disp.dimmed(),
    );
    println!(
        "{} Tables  {}   Functions  {}",
        "✔".green().bold(),
        n_tables.to_string().cyan(),
        n_funcs.to_string().cyan(),
    );

    // Fetch the TypeScript SDK source.
    print!("{} Generating SDK… ", "◆".blue());
    let sdk_source_raw = fetch_typescript_sdk(&client).await?;
    let sdk_source = prepend_header(
        &sdk_source_raw,
        project_id,
        version,
        &schema.schema_hash,
    );
    println!("\r{}                    ", " ".repeat(20)); // clear the spinner line

    // Write to disk.
    write_sdk(&sdk_source, &output_path).await?;

    println!(
        "{} SDK written → {}",
        "✔".green().bold(),
        output_path.display().to_string().cyan().bold()
    );

    println!();
    println!(
        "{}",
        format!(
            "  Add to your entry point:  import \"./{}\"",
            output_path.display()
        )
        .dimmed()
    );

    Ok(())
}

// ─── flux watch ───────────────────────────────────────────────────────────────

/// Execute `flux watch`.
///
/// `output`   — destination file path (defaults to `flux.generated.ts`).
/// `interval` — polling interval in seconds (defaults to 5).
pub async fn execute_watch(output: Option<String>, interval: u64) -> anyhow::Result<()> {
    let proj = ProjectConfig::load().await;
    let output_path = PathBuf::from(ProjectConfig::resolve_sdk_output(output, proj.as_ref()));
    let interval    = ProjectConfig::resolve_watch_interval(interval, proj.as_ref());
    let client = ApiClient::new().await?;
    let project_id = String::from("(use flux generate for types)");
    let mut last_hash = String::new();

    println!(
        "{} Watching schema (every {}s) → {}",
        "◉".blue().bold(),
        interval.to_string().yellow(),
        output_path.display().to_string().cyan()
    );

    // Ctrl-C handler — just let the OS kill us; the loop handles gracefully.
    let mut ticker = time::interval(time::Duration::from_secs(interval));
    // First tick fires immediately.
    loop {
        ticker.tick().await;

        let ts = now_hms().dimmed();

        let schema = match fetch_schema(&client).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  [{}] {} {}", ts, "✖".red(), e);
                continue;
            }
        };

        let version = schema.schema_version.unwrap_or(0);
        let hash    = schema.schema_hash.clone();

        if hash == last_hash {
            println!(
                "  [{}] {} {}  unchanged",
                ts,
                "✔".green(),
                format!("v{version}").dimmed(),
            );
            continue;
        }

        // Schema changed — regenerate.
        println!(
            "  [{}] {} {}  schema changed – regenerating…",
            ts,
            "↻".yellow().bold(),
            format!("v{version}").yellow().bold(),
        );

        match fetch_typescript_sdk(&client).await {
            Err(e) => {
                eprintln!("  [{}] {} Failed to fetch SDK: {}", ts, "✖".red(), e);
            }
            Ok(sdk_source_raw) => match write_sdk(
                    &prepend_header(&sdk_source_raw, &project_id, version, &hash),
                    &output_path,
                ).await {
                Err(e) => eprintln!("  [{}] {} Failed to write SDK: {}", ts, "✖".red(), e),
                Ok(()) => {
                        last_hash = hash;
                        let n_tables = schema.tables.as_ref().map(|t| t.len()).unwrap_or(0);
                        let n_funcs  = schema.functions.as_ref().map(|f| f.len()).unwrap_or(0);
                        println!(
                            "  [{}] {} SDK written → {}  ({} tables, {} functions)",
                            ts,
                            "✔".green().bold(),
                            output_path.display().to_string().cyan(),
                            n_tables.to_string().cyan(),
                            n_funcs.to_string().cyan(),
                        );
                    }
                },
        }
    }
}
// ─── flux status ──────────────────────────────────────────────────────────────

/// Execute `flux status`.
///
/// Compares the schema version embedded in the local SDK file against the
/// live remote schema, and reports whether the file is up-to-date.
///
/// `sdk_path` — path to the generated file (defaults to `flux.generated.ts`).
pub async fn execute_status(sdk_path: Option<String>) -> anyhow::Result<()> {
    let proj = ProjectConfig::load().await;
    let path = PathBuf::from(ProjectConfig::resolve_sdk_output(sdk_path, proj.as_ref()));
    let client = ApiClient::new().await?;

    let project_id = "(use flux generate for types)";
    println!("{} {}", "Project:".bold(), project_id.cyan());

    // ── Local ─────────────────────────────────────────────────────────────
    let local = if path.exists() {
        let src = fs::read_to_string(&path).await.unwrap_or_default();
        parse_local_version(&src)
    } else {
        None
    };

    match &local {
        Some((v, h)) => println!(
            "{} v{}  (hash: {})",
            "Local SDK:    ".bold(),
            v.to_string().yellow(),
            short_hash(h).dimmed(),
        ),
        None => println!(
            "{} {}",
            "Local SDK:    ".bold(),
            if path.exists() {
                "unrecognized format (no header)".yellow().to_string()
            } else {
                "not found".red().to_string()
            },
        ),
    }

    // ── Remote ────────────────────────────────────────────────────────────
    let remote = match fetch_schema(&client).await {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("{} Could not reach API: {}", "✖".red(), e);
            return Ok(());
        }
    };
    let remote_version = remote.schema_version.unwrap_or(0);
    println!(
        "{} v{}  (hash: {})",
        "Remote schema:".bold(),
        remote_version.to_string().yellow(),
        short_hash(&remote.schema_hash).dimmed(),
    );

    println!();

    // ── Comparison ────────────────────────────────────────────────────────
    match local {
        None => {
            println!("{}", "⚠  No local SDK found.".yellow().bold());
            println!("   Run: {}", "flux pull".cyan().bold());
        }
        Some((local_v, local_h)) => {
            let hash_match    = local_h == remote.schema_hash;
            let version_match = local_v == remote_version;

            if hash_match && version_match {
                println!("{}", "✔  SDK is up to date.".green().bold());
            } else {
                println!("{}", "⚠  SDK is out of date.".yellow().bold());
                if !version_match {
                    println!(
                        "   Local v{}  →  Remote v{}",
                        local_v.to_string().red(),
                        remote_version.to_string().green(),
                    );
                }
                if !hash_match {
                    println!("   Schema hash has changed.");
                }
                println!("   Run: {}", "flux pull".cyan().bold());
            }
        }
    }

    Ok(())
}