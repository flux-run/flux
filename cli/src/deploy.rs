use crate::client::ApiClient;
use colored::Colorize;
use reqwest::multipart;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

// ── Result type for a single-function deploy ──────────────────────────────

pub struct DeployResult {
    pub name: String,
    pub version: Option<u64>,
    pub url: Option<String>,
    pub error: Option<String>,
    pub elapsed_ms: u128,
}

// ── Bundle metadata extraction ────────────────────────────────────────────

/// Extract schema metadata embedded in a bundle by the defineFunction SDK.
/// The SDK serialises metadata as a JS object literal — we extract it by
/// running Node.js to `eval` and print the metadata as JSON.
fn extract_bundle_metadata(bundle_path: &Path) -> Option<Value> {
    // Quick extraction: look for the __fluxbase marker + metadata JSON in the bundle
    let bundle_content = fs::read_to_string(bundle_path).ok()?;

    // The SDK bakes metadata as a literal object: { name, description, input_schema, output_schema }
    // We run a tiny Node.js snippet to import the bundle and print its metadata
    let script = format!(
        r#"
import('file://{bundle}').then(m => {{
  const fn = m.default;
  if (fn && fn.__fluxbase) {{
    console.log(JSON.stringify(fn.metadata));
  }} else {{
    console.log('null');
  }}
}}).catch(() => console.log('null'));
"#,
        bundle = bundle_path.display()
    );

    let tmp = std::env::temp_dir().join("fluxbase_meta_extract.mjs");
    std::fs::write(&tmp, script).ok()?;

    let out = Command::new("node")
        .arg(&tmp)
        .output()
        .ok()?;

    // Also try to detect __fluxbase in the bundle text to fall back gracefully
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stdout != "null" && !stdout.is_empty() {
            return serde_json::from_str(&stdout).ok();
        }
    }

    // Fallback: detect if it's a framework bundle at all (has __fluxbase marker)
    if bundle_content.contains("__fluxbase") {
        return Some(serde_json::json!({ "name": null, "description": null, "input_schema": null, "output_schema": null }));
    }

    None
}

// ── Core: deploy a single function from a directory ───────────────────────

/// Bundle and upload one function whose root is at `dir`.
/// `force_name` / `force_runtime` override the values in `flux.json`.
async fn deploy_one_dir(
    dir: &Path,
    force_name: Option<&str>,
    force_runtime: Option<&str>,
    client: &ApiClient,
) -> anyhow::Result<DeployResult> {
    let t0 = Instant::now();

    let flux_json_path = dir.join("flux.json");
    let flux_json_content = fs::read_to_string(&flux_json_path)
        .map_err(|_| anyhow::anyhow!("flux.json not found in {}", dir.display()))?;
    let metadata: Value = serde_json::from_str(&flux_json_content)?;

    let name = force_name
        .map(String::from)
        .or_else(|| metadata["name"].as_str().map(String::from))
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "unknown".to_string());

    let runtime = force_runtime
        .map(String::from)
        .or_else(|| metadata["runtime"].as_str().map(String::from))
        .unwrap_or_else(|| "deno".to_string());

    let entry = metadata["entry"].as_str().unwrap_or("index.ts").to_string();

    // Enforce TypeScript-only
    if !entry.ends_with(".ts") {
        return Ok(DeployResult {
            name,
            version: None,
            url: None,
            error: Some(format!("entry '{}' must be a .ts file", entry)),
            elapsed_ms: t0.elapsed().as_millis(),
        });
    }

    let entry_path = dir.join(&entry);
    if !entry_path.exists() {
        return Ok(DeployResult {
            name,
            version: None,
            url: None,
            error: Some(format!("entry file '{}' not found", entry)),
            elapsed_ms: t0.elapsed().as_millis(),
        });
    }

    // Bundle with esbuild  (run from function dir so relative imports resolve)
    let out_dir = dir.join("dist");
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir)?;
    }
    let bundle_path = out_dir.join("bundle.js");

    let status = Command::new("npx")
        .args([
            "esbuild",
            &entry,
            "--bundle",
            "--platform=neutral",
            "--format=iife",
            "--global-name=__fluxbase_fn",
            &format!("--outfile={}", bundle_path.display()),
        ])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let ok = match status {
        Ok(s) if s.success() => true,
        _ => false,
    };

    if !ok {
        return Ok(DeployResult {
            name,
            version: None,
            url: None,
            error: Some("esbuild failed — run npm install in the function directory".to_string()),
            elapsed_ms: t0.elapsed().as_millis(),
        });
    }

    let bundle_content = fs::read(&bundle_path)?;

    // Extract schema metadata
    let fn_metadata = extract_bundle_metadata(&bundle_path);

    // Build multipart form
    let part = multipart::Part::bytes(bundle_content)
        .file_name("bundle.js")
        .mime_str("application/javascript")?;

    let mut form = multipart::Form::new()
        .text("name", name.clone())
        .text("runtime", runtime)
        .part("bundle", part);

    if let Some(ref meta) = fn_metadata {
        if let Some(desc) = meta.get("description").and_then(|d| d.as_str()) {
            form = form.text("description", desc.to_string());
        }
        if let Some(is) = meta.get("input_schema").filter(|v| !v.is_null()) {
            form = form.text("input_schema", is.to_string());
        }
        if let Some(os) = meta.get("output_schema").filter(|v| !v.is_null()) {
            form = form.text("output_schema", os.to_string());
        }
    }

    let res = client.deploy_function(form).await;

    let elapsed_ms = t0.elapsed().as_millis();

    match res {
        Ok(v) => {
            let data = v.get("data").unwrap_or(&v);
            Ok(DeployResult {
                name,
                version: data.get("version").and_then(|v| v.as_u64()),
                url: data.get("url").and_then(|u| u.as_str()).map(String::from),
                error: None,
                elapsed_ms,
            })
        }
        Err(e) => Ok(DeployResult {
            name,
            version: None,
            url: None,
            error: Some(format!("API error: {e}")),
            elapsed_ms,
        }),
    }
}

// ── Discover function directories ─────────────────────────────────────────

/// Walk immediate sub-directories of `root` and return those containing a flux.json.
fn discover_function_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return dirs };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("flux.json").exists() {
            dirs.push(path);
        }
    }
    dirs.sort();
    dirs
}

// ── Project-level deploy ──────────────────────────────────────────────────

pub async fn execute_project() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let function_dirs = discover_function_dirs(&cwd);

    if function_dirs.is_empty() {
        anyhow::bail!(
            "No function directories found.\n\
             \n\
             flux deploy expects either:\n\
             • A flux.json in the current directory (single-function deploy)\n\
             • Sub-directories containing flux.json files (project deploy)\n\
             \n\
             Run 'flux function create <name>' to scaffold a function."
        );
    }

    println!(
        "\n{} Deploying {} function{} to Fluxbase...\n",
        "▶".cyan().bold(),
        function_dirs.len().to_string().bold(),
        if function_dirs.len() == 1 { "" } else { "s" }
    );

    let client = ApiClient::new().await?;
    let total_t0 = Instant::now();

    let mut results: Vec<DeployResult> = Vec::new();

    for dir in &function_dirs {
        let fn_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".to_string());

        print!("  {} {}  ", "▸".dimmed(), fn_name.bold());
        // Flush so the name appears before bundling starts
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let result = deploy_one_dir(dir, None, None, &client).await?;

        match &result.error {
            None => {
                let ver = result.version.map(|v| format!("v{v}")).unwrap_or_default();
                println!(
                    "{} {} ({}ms)",
                    "✓".green().bold(),
                    ver.dimmed(),
                    result.elapsed_ms
                );
            }
            Some(err) => {
                println!("{} {}", "✗".red().bold(), err.red());
            }
        }

        results.push(result);
    }

    // ── Summary table ──────────────────────────────────────────────────────
    let total_ms = total_t0.elapsed().as_millis();
    let deployed = results.iter().filter(|r| r.error.is_none()).count();
    let failed = results.len() - deployed;

    println!();
    println!(
        "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
    println!(
        "  {:<24} {:>6}  {}",
        "Function".bold(),
        "Ver".bold(),
        "URL".bold()
    );
    println!(
        "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
    for r in &results {
        if let Some(ref err) = r.error {
            println!(
                "  {:<24} {:>6}  {}",
                r.name.red(),
                "—",
                err.red()
            );
        } else {
            let ver = r.version.map(|v| format!("v{v}")).unwrap_or("—".to_string());
            let url = r.url.as_deref().unwrap_or("—");
            println!("  {:<24} {:>6}  {}", r.name.green(), ver.dimmed(), url);
        }
    }
    println!(
        "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
    println!();

    if failed == 0 {
        println!(
            "  {} {} function{} deployed in {}ms",
            "✓".green().bold(),
            deployed.to_string().bold(),
            if deployed == 1 { "" } else { "s" },
            total_ms
        );
    } else {
        println!(
            "  {} {} deployed  {} failed  ({}ms)",
            "⚠".yellow().bold(),
            deployed.to_string().green().bold(),
            failed.to_string().red().bold(),
            total_ms
        );
    }
    println!();

    Ok(())
}

// ── Single-function entry point (unchanged public API) ────────────────────

pub async fn execute(
    arg_name: Option<String>,
    arg_runtime: Option<String>,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    // Auto-detect: no flux.json in cwd → project deploy
    if !cwd.join("flux.json").exists() {
        return execute_project().await;
    }

    // ── Single-function deploy ─────────────────────────────────────────────
    let flux_json_path = Path::new("flux.json");
    let flux_json_content = fs::read_to_string(flux_json_path)?;
    let metadata: serde_json::Value = serde_json::from_str(&flux_json_content)?;

    let name = arg_name
        .or_else(|| metadata["name"].as_str().map(String::from))
        .or_else(|| {
            // Derive name from the current directory name
            std::env::current_dir().ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "unknown".to_string());

    let runtime = arg_runtime
        .or_else(|| metadata["runtime"].as_str().map(String::from))
        .unwrap_or_else(|| "deno".to_string());

    // Delegate to the shared core (verbose output for single-function mode)
    println!("\n  {} Bundling {} ({})...", "▸".cyan(), name.bold(), "flux.json".dimmed());

    let client = ApiClient::new().await?;
    let result = deploy_one_dir(&cwd, Some(&name), Some(&runtime), &client).await?;

    match result.error {
        Some(ref err) => anyhow::bail!("{}", err),
        None => {
            println!("\n  {} Function deployed successfully!\n", "✓".green().bold());
            if let Some(ref url) = result.url {
                println!("     URL:      {}", url.cyan());
            }
            if let Some(ver) = result.version {
                println!("     Version:  {}", format!("v{ver}").dimmed());
            }
            println!("     Time:     {}ms", result.elapsed_ms);
            println!();
            println!("  Test with:  flux invoke {}", name.bold());
            println!();
        }
    }

    Ok(())
}
