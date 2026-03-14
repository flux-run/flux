//! `flux deploy` — bundle and upload functions, with hash-based incremental
//! deploys and project-level deployment records.
//!
//! ## Flow
//! 1. Resolve context (endpoint + API key)
//! 2. Scan `functions/` directory (or CWD) for sub-dirs containing `flux.json`
//! 3. Optionally filter by `--only`
//! 4. Unless `--force`, fetch server hashes via `GET /api/deployments/hashes`
//! 5. Bundle each function, compute SHA-256 of bundle bytes
//! 6. Skip functions whose hash matches the server hash
//! 7. Upload changed functions via `POST /api/functions/deploy` (multipart)
//! 8. Record the project-level deployment via `POST /api/deployments/project`
//! 9. Print summary

use colored::Colorize;
use reqwest::multipart;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::context::{resolve_context, ResolvedContext};

// ── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DeployStatus {
    Deployed { version: u64, url: Option<String> },
    Skipped,
    Failed(String),
}

pub struct FunctionResult {
    pub name:       String,
    pub status:     DeployStatus,
    pub elapsed_ms: u128,
}

// ── Bundle output ─────────────────────────────────────────────────────────────

pub enum BundleKind {
    Js  { metadata: Option<Value> },
    Wasm,
}

pub struct BundleOutput {
    pub bytes:   Vec<u8>,
    pub runtime: String,
    pub kind:    BundleKind,
}

// ── Hash ──────────────────────────────────────────────────────────────────────

fn hash_bundle(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

// ── Bundle metadata extraction ────────────────────────────────────────────────

fn extract_bundle_metadata(bundle_path: &Path) -> Option<Value> {
    let bundle_content = fs::read_to_string(bundle_path).ok()?;

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

    let out = Command::new("node").arg(&tmp).output().ok()?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stdout != "null" && !stdout.is_empty() {
            return serde_json::from_str(&stdout).ok();
        }
    }

    if bundle_content.contains("__fluxbase") {
        return Some(serde_json::json!({
            "name": null, "description": null,
            "input_schema": null, "output_schema": null
        }));
    }

    None
}

// ── WASM toolchain pre-flight ─────────────────────────────────────────────────

fn check_wasm_toolchain(build_cmd: &str) -> Option<String> {
    struct Check {
        needle:  &'static str,
        binary:  &'static str,
        install: &'static str,
    }

    let checks: &[Check] = &[
        Check { needle: "tinygo",      binary: "tinygo",  install: "https://tinygo.org/getting-started/install/" },
        Check { needle: "asc ",        binary: "asc",     install: "npm install -g assemblyscript  (https://www.assemblyscript.org)" },
        Check { needle: "npx asc",     binary: "npx",     install: "https://nodejs.org" },
        Check { needle: "zig build",   binary: "zig",     install: "https://ziglang.org/download/" },
        Check { needle: "py2wasm",     binary: "py2wasm", install: "pip install py2wasm  (https://github.com/astral-sh/py2wasm)" },
        Check { needle: "emcc",        binary: "emcc",    install: "https://emscripten.org/docs/getting_started/downloads.html" },
        Check { needle: "cargo build", binary: "cargo",   install: "rustup  (https://rustup.rs)" },
    ];

    for check in checks {
        if build_cmd.contains(check.needle) {
            if which::which(check.binary).is_err() {
                return Some(format!(
                    "required toolchain '{}' not found on PATH.\nInstall it from: {}",
                    check.binary, check.install,
                ));
            }
            if check.binary == "cargo" && build_cmd.contains("wasm32-wasip1") {
                let targets = Command::new("rustup")
                    .args(["target", "list", "--installed"])
                    .output()
                    .ok();
                let has_target = targets
                    .as_ref()
                    .and_then(|o| String::from_utf8(o.stdout.clone()).ok())
                    .map(|s| s.contains("wasm32-wasip1"))
                    .unwrap_or(false);
                if !has_target {
                    return Some(
                        "wasm32-wasip1 target not installed.\nRun: rustup target add wasm32-wasip1"
                            .to_string(),
                    );
                }
            }
        }
    }
    None
}

// ── Bundle: JS/Deno ───────────────────────────────────────────────────────────

fn bundle_js(dir: &Path, entry: &str) -> anyhow::Result<Vec<u8>> {    let out_dir = dir.join("dist");
    if !out_dir.exists() {
        fs::create_dir_all(&out_dir)?;
    }
    let bundle_path = out_dir.join("bundle.js");

    let status = Command::new("npx")
        .args([
            "esbuild",
            entry,
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

    match status {
        Ok(s) if s.success() => {}
        _ => anyhow::bail!("esbuild failed — run npm install in the function directory"),
    }

    Ok(fs::read(&bundle_path)?)
}

// ── Bundle: WASM ──────────────────────────────────────────────────────────────

fn bundle_wasm(dir: &Path, entry: &str, build_cmd: Option<&str>) -> anyhow::Result<Vec<u8>> {
    if let Some(cmd) = build_cmd {
        if let Some(hint) = check_wasm_toolchain(cmd) {
            anyhow::bail!("{}", hint);
        }
        let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
        let flag  = if cfg!(target_os = "windows") { "/C" } else { "-c" };
        let status = Command::new(shell).args([flag, cmd]).current_dir(dir).status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => anyhow::bail!("build command failed (exit {})", s),
            Err(e) => anyhow::bail!("could not run build command: {}", e),
        }
    }

    let wasm_path = dir.join(entry);
    if !wasm_path.exists() {
        anyhow::bail!(
            "WASM binary '{}' not found. Set \"build\" in flux.json or build manually.",
            entry
        );
    }

    let bytes = fs::read(&wasm_path)?;
    if bytes.len() < 8 || &bytes[0..4] != b"\x00asm" {
        anyhow::bail!(
            "'{}' is not a valid WASM binary (wrong magic bytes).",
            entry
        );
    }
    Ok(bytes)
}

// ── Bundle dispatch ───────────────────────────────────────────────────────────

pub fn bundle_function(dir: &Path, metadata: &Value) -> anyhow::Result<BundleOutput> {
    let runtime = metadata["runtime"].as_str().unwrap_or("deno").to_string();

    // Default entry: for wasm → handler.wasm; for JS/TS → index.ts (prefer .js fallback)
    let entry = metadata["entry"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if runtime == "wasm" {
                "handler.wasm".to_string()
            } else if dir.join("index.js").exists() && !dir.join("index.ts").exists() {
                "index.js".to_string()
            } else {
                "index.ts".to_string()
            }
        });

    if runtime == "wasm" {
        let build_cmd = metadata["build"].as_str();
        let bytes = bundle_wasm(dir, &entry, build_cmd)?;
        return Ok(BundleOutput { bytes, runtime, kind: BundleKind::Wasm });
    }

    if !entry.ends_with(".ts") && !entry.ends_with(".js") {
        anyhow::bail!("entry '{}' must be a .ts or .js file", entry);
    }
    if !dir.join(&entry).exists() {
        anyhow::bail!("entry file '{}' not found", entry);
    }

    let bytes = bundle_js(dir, &entry)?;
    let out_bundle_path = dir.join("dist").join("bundle.js");
    let extracted_meta = extract_bundle_metadata(&out_bundle_path);

    Ok(BundleOutput {
        bytes,
        runtime,
        kind: BundleKind::Js { metadata: extracted_meta },
    })
}

// ── Upload a single function ──────────────────────────────────────────────────

async fn upload_function(
    ctx:                   &ResolvedContext,
    name:                  &str,
    bundle:                BundleOutput,
    bundle_hash:           &str,
    project_deployment_id: Option<&str>,
) -> anyhow::Result<(u64, Option<String>)> {
    let client = reqwest::Client::new();

    let mime = if bundle.runtime == "wasm" { "application/wasm" } else { "application/javascript" };
    let file_name = if bundle.runtime == "wasm" { "handler.wasm" } else { "bundle.js" };

    let part = multipart::Part::bytes(bundle.bytes)
        .file_name(file_name)
        .mime_str(mime)?;

    let mut form = multipart::Form::new()
        .text("name",        name.to_string())
        .text("runtime",     bundle.runtime.clone())
        .text("bundle_hash", bundle_hash.to_string())
        .part("bundle",      part);

    if let Some(pdid) = project_deployment_id {
        form = form.text("project_deployment_id", pdid.to_string());
    }

    // Attach schema metadata extracted from JS bundles.
    if let BundleKind::Js { metadata: Some(ref meta) } = bundle.kind {
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

    let url = format!("{}/flux/api/functions/deploy", ctx.endpoint);
    let mut req = client.post(&url).multipart(form);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?.error_for_status()?;
    let json: Value = resp.json().await?;
    let data = json.get("data").unwrap_or(&json);

    let version = data.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let run_url = data
        .get("run_url")
        .or_else(|| data.get("url"))
        .and_then(|u| u.as_str())
        .map(String::from);

    Ok((version, run_url))
}

// ── Fetch server-side hashes ──────────────────────────────────────────────────

async fn fetch_server_hashes(ctx: &ResolvedContext) -> anyhow::Result<HashMap<String, String>> {
    let client = reqwest::Client::new();
    let url = format!("{}/flux/api/deployments/hashes", ctx.endpoint);
    let mut req = client.get(&url);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        // Gracefully degrade — treat as no hashes (force full deploy).
        return Ok(HashMap::new());
    }
    let json: Value = resp.json().await?;
    let hashes = json
        .get("data")
        .and_then(|d| d.get("hashes"))
        .or_else(|| json.get("hashes"))
        .and_then(|h| h.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Ok(hashes)
}

// ── Record project deployment ─────────────────────────────────────────────────

async fn record_project_deployment(
    ctx:     &ResolvedContext,
    version: u64,
    results: &[FunctionResult],
) -> anyhow::Result<String> {
    let deployed = results
        .iter()
        .filter(|r| matches!(r.status, DeployStatus::Deployed { .. }))
        .count() as i64;
    let skipped = results
        .iter()
        .filter(|r| r.status == DeployStatus::Skipped)
        .count() as i64;

    let functions: Vec<Value> = results
        .iter()
        .map(|r| {
            let (fn_version, status_str) = match &r.status {
                DeployStatus::Deployed { version, .. } => (*version as i64, "deployed"),
                DeployStatus::Skipped                  => (0,              "skipped"),
                DeployStatus::Failed(_)                => (0,              "failed"),
            };
            serde_json::json!({
                "name":    r.name,
                "version": fn_version,
                "status":  status_str,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "version": version,
        "summary": {
            "total":     results.len() as i64,
            "deployed":  deployed,
            "skipped":   skipped,
            "functions": functions,
        },
        "deployed_by": "cli",
    });

    let client = reqwest::Client::new();
    let url = format!("{}/flux/api/deployments/project", ctx.endpoint);
    let mut req = client.post(&url).json(&payload);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?.error_for_status()?;
    let json: Value = resp.json().await?;
    let data = json.get("data").unwrap_or(&json);
    let id = data
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(id)
}

// ── Discover function directories ─────────────────────────────────────────────

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

// ── Print helpers ─────────────────────────────────────────────────────────────

fn print_summary(results: &[FunctionResult], project_version: u64, ctx: &ResolvedContext) {
    let deployed = results.iter().filter(|r| matches!(r.status, DeployStatus::Deployed { .. })).count();
    let skipped  = results.iter().filter(|r| r.status == DeployStatus::Skipped).count();
    let failed   = results.iter().filter(|r| matches!(r.status, DeployStatus::Failed(_))).count();

    println!();
    println!("  {} Deployed  project v{}", "✔".green().bold(), project_version.to_string().bold());
    println!(
        "    {} deployed · {} skipped · {} failed",
        deployed.to_string().green().bold(),
        skipped.to_string().dimmed(),
        if failed > 0 { failed.to_string().red().bold() } else { failed.to_string().dimmed() },
    );
    println!();

    for r in results {
        match &r.status {
            DeployStatus::Deployed { version, url: _ } => {
                println!(
                    "    {} {}   v{}  {}",
                    "↑".green().bold(),
                    format!("{:<20}", r.name).green(),
                    version,
                    format!("{}ms", r.elapsed_ms).dimmed(),
                );
            }
            DeployStatus::Skipped => {
                println!(
                    "    {} {}   {}",
                    "─".dimmed(),
                    format!("{:<20}", r.name).dimmed(),
                    "skipped (unchanged)".dimmed(),
                );
            }
            DeployStatus::Failed(err) => {
                println!(
                    "    {} {}   {}",
                    "✗".red().bold(),
                    format!("{:<20}", r.name).red(),
                    err.red(),
                );
            }
        }
    }
    println!();

    // Show a helpful invoke hint for the first successfully deployed function.
    if let Some(first_deployed) = results.iter().find(|r| matches!(r.status, DeployStatus::Deployed { .. })) {
        println!("  Run:  flux invoke {}", first_deployed.name.bold());
    }
    println!("  Dash: {}/flux", ctx.endpoint.dimmed());
    println!();
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Deploy all (or selected) functions in the project.
///
/// * `context_name` — `--context` flag; falls back to `resolve_context` defaults
/// * `only`         — `--only` flag; if set, only deploy functions with these names
/// * `force`        — `--force` flag; skip hash check, redeploy everything
pub async fn execute(
    context_name: Option<String>,
    only:         Option<Vec<String>>,
    force:        bool,
) -> anyhow::Result<()> {
    let project_root = crate::dev::find_project_root_pub();
    let ctx = resolve_context(context_name.as_deref(), project_root.as_deref())?;

    let cwd = std::env::current_dir()?;

    // Check if we're inside a single function directory.
    if cwd.join("flux.json").exists() && only.is_none() {
        return execute_single_dir(&cwd, &ctx, force).await;
    }

    // Project-level deploy: scan `functions/` subdir, or the CWD itself.
    let scan_root = if cwd.join("functions").is_dir() {
        cwd.join("functions")
    } else {
        cwd.clone()
    };

    let mut function_dirs = discover_function_dirs(&scan_root);

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

    // Apply --only filter.
    if let Some(ref names) = only {
        function_dirs.retain(|d| {
            d.file_name()
                .map(|n| names.iter().any(|want| want == n.to_string_lossy().as_ref()))
                .unwrap_or(false)
        });
        if function_dirs.is_empty() {
            anyhow::bail!("No matching functions found for --only {:?}", names);
        }
    }

    println!();
    println!("  {} Deploying to {} ({})", "◆".cyan().bold(), ctx.name.cyan().bold(), ctx.endpoint.dimmed());
    println!();
    println!("  Scanning functions/…    {} function(s) found", function_dirs.len());
    println!();

    // Fetch server hashes for incremental deploys.
    let server_hashes: HashMap<String, String> = if force {
        HashMap::new()
    } else {
        println!("  Checking for changes…");
        match fetch_server_hashes(&ctx).await {
            Ok(h) => h,
            Err(_) => {
                println!("    {} could not fetch hashes — deploying all", "⚠".yellow());
                HashMap::new()
            }
        }
    };

    println!();
    println!("  Bundling changed functions…");

    let t0 = Instant::now();
    let mut results: Vec<FunctionResult> = Vec::new();

    for dir in &function_dirs {
        let fn_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());

        let flux_json_path = dir.join("flux.json");
        let flux_json_content = match fs::read_to_string(&flux_json_path) {
            Ok(s) => s,
            Err(_) => {
                results.push(FunctionResult {
                    name:       fn_name,
                    status:     DeployStatus::Failed("flux.json not found".into()),
                    elapsed_ms: 0,
                });
                continue;
            }
        };
        let fn_metadata: Value = match serde_json::from_str(&flux_json_content) {
            Ok(v) => v,
            Err(e) => {
                results.push(FunctionResult {
                    name:       fn_name,
                    status:     DeployStatus::Failed(format!("invalid flux.json: {e}")),
                    elapsed_ms: 0,
                });
                continue;
            }
        };

        let name = fn_metadata["name"]
            .as_str()
            .unwrap_or(&fn_name)
            .to_string();

        let fn_t0 = Instant::now();

        print!("    {} {}   bundling…", "↑".cyan(), format!("{:<20}", name));

        let bundle = match bundle_function(dir, &fn_metadata) {
            Ok(b) => b,
            Err(e) => {
                println!("  {}", "✗".red().bold());
                results.push(FunctionResult {
                    name,
                    status:     DeployStatus::Failed(e.to_string()),
                    elapsed_ms: fn_t0.elapsed().as_millis(),
                });
                continue;
            }
        };

        let local_hash = hash_bundle(&bundle.bytes);

        // Skip if hash matches server (and not forced).
        if !force {
            if let Some(server_hash) = server_hashes.get(&name) {
                if *server_hash == local_hash {
                    println!("  {}", "skipped (unchanged)".dimmed());
                    results.push(FunctionResult {
                        name,
                        status:     DeployStatus::Skipped,
                        elapsed_ms: fn_t0.elapsed().as_millis(),
                    });
                    continue;
                }
            }
        }

        // Upload the function.
        match upload_function(&ctx, &name, bundle, &local_hash, None).await {
            Ok((version, run_url)) => {
                println!("  {}  v{}   (hash changed)", "✔".green().bold(), version);
                results.push(FunctionResult {
                    name,
                    status:     DeployStatus::Deployed { version, url: run_url },
                    elapsed_ms: fn_t0.elapsed().as_millis(),
                });
            }
            Err(e) => {
                println!("  {}", "✗".red().bold());
                results.push(FunctionResult {
                    name,
                    status:     DeployStatus::Failed(format!("upload failed: {e}")),
                    elapsed_ms: fn_t0.elapsed().as_millis(),
                });
            }
        }
    }

    let total_ms = t0.elapsed().as_millis();
    let _ = total_ms;

    let deployed_count = results.iter().filter(|r| matches!(r.status, DeployStatus::Deployed { .. })).count();

    // Derive next project version (simple heuristic: max function version + 1).
    let max_version = results.iter().filter_map(|r| {
        if let DeployStatus::Deployed { version, .. } = r.status { Some(version) } else { None }
    }).max().unwrap_or(0);
    let project_version = max_version;

    // Record the project deployment if anything was deployed.
    if deployed_count > 0 {
        let _ = record_project_deployment(&ctx, project_version, &results).await;
    }

    // Sync routes from flux.toml (if present).
    let project_root = cwd.clone();
    print!("  {} routes           syncing…", "↑".blue().bold());
    match sync_routes(&ctx, None, &project_root).await {
        Ok(n)  => println!("\r  {} routes           {} route(s) synced   ", "✔".green().bold(), n),
        Err(e) => println!("\r  {} routes           skipped ({})", "─".dimmed(), e),
    }

    // Auto-regenerate .flux/ type stubs so editors are immediately up-to-date.
    print!("  {} generate        regenerating types…", "↑".blue().bold());
    match crate::generate::execute_generate(None).await {
        Ok(()) => println!("\r  {} generate        .flux/ updated            ", "✔".green().bold()),
        Err(e) => println!("\r  {} generate        skipped ({})", "─".dimmed(), e),
    }

    print_summary(&results, project_version, &ctx);

    Ok(())
}

// ── Route sync ────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
struct FluxTomlRoutes {
    routes: Option<Vec<RouteEntry>>,
}

#[derive(serde::Deserialize)]
struct RouteEntry {
    path:       String,
    #[serde(default = "default_method")]
    method:     String,
    function:   String,
    #[serde(default)]
    middleware: Vec<String>,
    rate_limit: Option<u32>,
}

fn default_method() -> String { "POST".into() }

async fn sync_routes(
    ctx:                   &ResolvedContext,
    project_deployment_id: Option<&str>,
    project_root:          &Path,
) -> anyhow::Result<usize> {
    let toml_path = project_root.join("flux.toml");
    let toml_str = fs::read_to_string(&toml_path)
        .map_err(|_| anyhow::anyhow!("flux.toml not found"))?;

    let parsed: FluxTomlRoutes = toml::from_str(&toml_str)
        .map_err(|e| anyhow::anyhow!("failed to parse flux.toml: {e}"))?;

    let entries = match parsed.routes {
        Some(r) if !r.is_empty() => r,
        _ => {
            println!("  {} routes           no [[routes]] in flux.toml — skipping", "─".dimmed());
            return Ok(0);
        }
    };

    let routes_json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::json!({
            "path":                  e.path,
            "method":                e.method,
            "function_name":         e.function,
            "middleware":            e.middleware,
            "rate_limit_per_minute": e.rate_limit,
        }))
        .collect();

    let payload = serde_json::json!({
        "project_deployment_id": project_deployment_id,
        "routes": routes_json,
    });

    let client = reqwest::Client::new();
    let url = format!("{}/flux/api/routes/sync", ctx.endpoint);
    let mut req = client.post(&url).json(&payload);
    if !ctx.api_key.is_empty() {
        req = req.bearer_auth(&ctx.api_key);
    }

    let resp = req.send().await?.error_for_status()?;
    let json: serde_json::Value = resp.json().await?;
    let count = json
        .get("data").unwrap_or(&json)
        .get("synced")
        .and_then(|v| v.as_u64())
        .unwrap_or(entries.len() as u64) as usize;

    Ok(count)
}

// ── Single-function deploy (called when CWD has flux.json) ────────────────────

async fn execute_single_dir(
    dir:   &Path,
    ctx:   &ResolvedContext,
    force: bool,
) -> anyhow::Result<()> {
    let flux_json_path = dir.join("flux.json");
    let flux_json_content = fs::read_to_string(&flux_json_path)?;
    let fn_metadata: Value = serde_json::from_str(&flux_json_content)?;

    let name = fn_metadata["name"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            dir.file_name().map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".into());

    println!();
    println!(
        "  {} Deploying {} to {} ({})",
        "◆".cyan().bold(),
        name.bold(),
        ctx.name.cyan().bold(),
        ctx.endpoint.dimmed()
    );
    println!();
    print!("  Bundling {}…", name.bold());

    let t0 = Instant::now();

    let bundle = bundle_function(dir, &fn_metadata)?;
    let local_hash = hash_bundle(&bundle.bytes);

    // Check for hash match unless forced.
    if !force {
        let server_hashes = fetch_server_hashes(ctx).await.unwrap_or_default();
        if let Some(server_hash) = server_hashes.get(&name) {
            if *server_hash == local_hash {
                println!("  {} {} is unchanged — skipping", "─".dimmed(), name.dimmed());
                println!();
                println!("  Use --force to redeploy anyway.");
                println!();
                return Ok(());
            }
        }
    }

    println!("  {}", "✔".green().bold());

    let (version, run_url) = upload_function(ctx, &name, bundle, &local_hash, None).await?;
    let elapsed_ms = t0.elapsed().as_millis();

    println!();
    println!("  {} Function deployed successfully!\n", "✓".green().bold());
    if let Some(ref url) = run_url {
        println!("     URL:      {}", url.cyan());
    }
    println!("     Version:  {}", format!("v{version}").dimmed());
    println!("     Time:     {}ms", elapsed_ms);
    println!();
    println!("  Test with:  flux invoke {}", name.bold());
    println!();

    Ok(())
}
