use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::build::{FluxManifest, content_hash, detect_features};

// ─── CLI args ────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct DevArgs {
    /// Entry file (JS or TS).
    #[arg(value_name = "ENTRY", default_value = "index.ts")]
    pub entry: String,

    /// Flux server URL for recording executions (optional).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Service token for the Flux server (optional).
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Bind host for the HTTP server.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// HTTP port.
    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    /// Number of isolates in the pool.
    #[arg(long, default_value_t = 1)]
    pub isolate_pool_size: usize,

    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,

    /// File-change poll interval in milliseconds.
    #[arg(long, default_value_t = 500)]
    pub poll_ms: u64,

    /// Directory to watch (defaults to the directory containing ENTRY).
    #[arg(long)]
    pub watch_dir: Option<String>,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub async fn execute(args: DevArgs) -> Result<()> {
    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    let workspace_root = find_workspace_root()
        .ok_or_else(|| anyhow::anyhow!("could not locate workspace root containing Cargo.toml"))?;

    let binary = find_runtime_binary(&workspace_root, args.release);

    // Server URL and token are optional in dev mode.
    let server_url = args
        .url
        .clone()
        .or_else(read_config_url)
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
    let token = args.token.clone().or_else(read_config_token).unwrap_or_default();

    let watch_dir = args
        .watch_dir
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| entry.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    eprintln!(
        "flux dev  {} (watching {}, poll {}ms)",
        args.entry,
        watch_dir.display(),
        args.poll_ms,
    );
    eprintln!("          Ctrl+C to stop\n");

    loop {
        refresh_manifest(&entry)?;

        let mut child = spawn_runtime(&workspace_root, &binary, &server_url, &token, &args)?;
        eprintln!("[flux dev] started  pid {:?}", child.id());

        let mtime_before = dir_mtime(&watch_dir);

        let should_restart = loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(args.poll_ms)).await;

            match child.try_wait() {
                Ok(Some(status)) => {
                    eprintln!("[flux dev] runtime exited ({status}), restarting…");
                    break true;
                }
                Ok(None) => {} // still running
                Err(e) => {
                    eprintln!("[flux dev] wait error: {e}, restarting…");
                    break true;
                }
            }

            if dir_mtime(&watch_dir) != mtime_before {
                eprintln!("[flux dev] change detected, restarting…");
                break true;
            }
        };

        if should_restart {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        // Brief pause so the OS can release the port before the next spawn.
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }
}

// ─── Manifest refresh ────────────────────────────────────────────────────────

/// Write (or overwrite) `flux.json` beside the entry file with the current
/// feature set. Called before each runtime spawn so the manifest stays fresh.
fn refresh_manifest(entry: &PathBuf) -> Result<()> {
    let source = std::fs::read_to_string(entry)
        .with_context(|| format!("failed to read {}", entry.display()))?;

    let manifest = FluxManifest {
        flux_version: "0.2".to_string(),
        entry: entry.to_string_lossy().into_owned(),
        code_hash: content_hash(&source),
        built_at: chrono::Utc::now().to_rfc3339(),
        runtime_features: detect_features(&source).into_iter().collect(),
        bundled: None,
        minified: false,
    };

    let json =
        serde_json::to_string_pretty(&manifest).context("failed to serialise flux.json")?;
    let out = entry.parent().unwrap_or(Path::new(".")).join("flux.json");
    std::fs::write(&out, json).with_context(|| format!("failed to write {}", out.display()))
}

// ─── Runtime spawning ────────────────────────────────────────────────────────

fn spawn_runtime(
    workspace_root: &Path,
    binary: &Option<PathBuf>,
    server_url: &str,
    token: &str,
    args: &DevArgs,
) -> Result<tokio::process::Child> {
    let prog_args = build_runtime_args(server_url, token, args);

    if let Some(bin) = binary {
        tokio::process::Command::new(bin)
            .args(&prog_args)
            .spawn()
            .context("failed to spawn flux-runtime")
    } else {
        let mut cmd = tokio::process::Command::new("cargo");
        cmd.current_dir(workspace_root)
            .args(["run", "-p", "runtime", "--bin", "flux-runtime"]);
        if args.release {
            cmd.arg("--release");
        }
        cmd.arg("--").args(&prog_args);
        cmd.spawn()
            .context("failed to spawn flux-runtime via `cargo run`")
    }
}

fn build_runtime_args(server_url: &str, token: &str, args: &DevArgs) -> Vec<String> {
    vec![
        "--entry".to_string(),
        args.entry.clone(),
        "--server-url".to_string(),
        server_url.to_string(),
        "--token".to_string(),
        token.to_string(),
        "--host".to_string(),
        args.host.clone(),
        "--port".to_string(),
        args.port.to_string(),
        "--isolate-pool-size".to_string(),
        args.isolate_pool_size.to_string(),
    ]
}

// ─── File watching ───────────────────────────────────────────────────────────

/// Return the most-recent mtime of any JS/TS/JSON file directly inside `dir`
/// (one level — skips `node_modules` and hidden directories).
fn dir_mtime(dir: &Path) -> Option<SystemTime> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|res| {
            let entry = res.ok()?;
            let path = entry.path();

            let name = path.file_name()?.to_string_lossy().into_owned();
            if path.is_dir() && (name == "node_modules" || name.starts_with('.')) {
                return None;
            }

            let ext = path.extension()?.to_string_lossy().into_owned();
            if matches!(ext.as_str(), "js" | "ts" | "jsx" | "tsx" | "json") {
                entry.metadata().ok()?.modified().ok()
            } else {
                None
            }
        })
        .max()
}

// ─── Local helpers (mirrors run.rs / serve.rs) ───────────────────────────────

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let contents = std::fs::read_to_string(&cargo_toml).ok()?;
            if contents.contains("[workspace]") {
                return Some(dir);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn find_runtime_binary(workspace_root: &Path, release: bool) -> Option<PathBuf> {
    let name = if cfg!(windows) { "flux-runtime.exe" } else { "flux-runtime" };
    let primary = if release { "release" } else { "debug" };
    let secondary = if release { "debug" } else { "release" };

    [primary, secondary]
        .into_iter()
        .map(|profile| workspace_root.join("target").join(profile).join(name))
        .find(|p| p.exists())
}

fn flux_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("config.toml")
}

fn read_config_url() -> Option<String> {
    let raw = std::fs::read_to_string(flux_config_path()).ok()?;
    raw.lines()
        .find(|l| l.starts_with("url"))
        .and_then(|l| l.splitn(2, '=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
}

fn read_config_token() -> Option<String> {
    let raw = std::fs::read_to_string(flux_config_path()).ok()?;
    raw.lines()
        .find(|l| l.starts_with("token"))
        .and_then(|l| l.splitn(2, '=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
}
