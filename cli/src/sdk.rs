//! `flux pull` and `flux watch` — SDK code generation commands.
//!
//! # Pull
//!
//! Fetches the live schema from your Fluxbase project and writes a fully-typed
//! TypeScript SDK file:
//!
//! ```text
//! $ flux pull
//! ✔ Connected to project  proj_abc123
//! ✔ Schema  v4  (hash: a3f8c1d2)
//! ✔ Tables  8   Functions  3
//! ✔ SDK written → src/fluxbase.generated.ts
//! ```
//!
//! # Watch
//!
//! Polls the schema hash every N seconds (default 5) and regenerates the SDK
//! file whenever the schema changes:
//!
//! ```text
//! $ flux watch --interval 10
//! ◉ Watching schema (every 10s) → fluxbase.generated.ts
//!   [10:02:31] ✔ v4  unchanged
//!   [10:02:41] ↻ v5  schema changed – regenerating…
//!   [10:02:41] ✔ SDK written → fluxbase.generated.ts
//! ```

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use colored::Colorize;
use serde::Deserialize;
use tokio::{fs, time};

use crate::client::ApiClient;

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
        .context("Failed to reach Fluxbase API")?;

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
        .context("Failed to reach Fluxbase API")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("API returned {status}: {body}");
    }

    res.text().await.context("Failed to read SDK response body")
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

fn now_hms() -> String {
    // Use a simple epoch-based approximation since we avoid pulling in chrono.
    // In practice tokio's clock is fine for display purposes.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

// ─── flux pull ────────────────────────────────────────────────────────────────

/// Execute `flux pull`.
///
/// `output` — destination file path (defaults to `fluxbase.generated.ts`).
pub async fn execute_pull(output: Option<String>) -> anyhow::Result<()> {
    let output_path = PathBuf::from(output.unwrap_or_else(|| "fluxbase.generated.ts".into()));

    let client = ApiClient::new().await?;

    // Print the project context so the user sees what they're pulling from.
    let project_id = client.config.project_id.as_deref().unwrap_or("(no project set)");
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
    let sdk_source = fetch_typescript_sdk(&client).await?;
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
/// `output`   — destination file path (defaults to `fluxbase.generated.ts`).
/// `interval` — polling interval in seconds (defaults to 5).
pub async fn execute_watch(output: Option<String>, interval: u64) -> anyhow::Result<()> {
    let output_path = PathBuf::from(output.unwrap_or_else(|| "fluxbase.generated.ts".into()));
    let client = ApiClient::new().await?;
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
            Ok(sdk_source) => match write_sdk(&sdk_source, &output_path).await {
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
