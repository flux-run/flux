use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Entry file to execute as a plain script.
    #[arg(value_name = "ENTRY", default_value = "index.js")]
    pub entry: String,

    /// Flux server URL for recording the execution (optional).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Service token for the Flux server (optional).
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,
}

pub async fn execute(args: RunArgs) -> Result<()> {
    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    let workspace_root = find_workspace_root()
        .ok_or_else(|| anyhow::anyhow!("could not locate workspace root containing Cargo.toml"))?;

    let binary = find_runtime_binary(&workspace_root, args.release);

    // Server URL and token are optional for script mode — default to empty
    // strings so the runtime can start without a running flux-server.
    let server_url = args
        .url
        .clone()
        .or_else(|| read_config_url())
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
    let token = args
        .token
        .clone()
        .or_else(|| read_config_token())
        .unwrap_or_default();

    start_script(workspace_root, binary, &server_url, &token, &args).await
}

#[cfg(unix)]
async fn start_script(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    server_url: &str,
    token: &str,
    args: &RunArgs,
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let prog_args = build_runtime_args(server_url, token, args);

    let err = if let Some(bin) = binary {
        std::process::Command::new(bin).args(&prog_args).exec()
    } else {
        let mut cmd = std::process::Command::new("cargo");
        cmd.current_dir(&workspace_root)
            .args(["run", "-p", "runtime", "--bin", "flux-runtime"]);
        if args.release {
            cmd.arg("--release");
        }
        cmd.arg("--").args(&prog_args).exec()
    };

    bail!("failed to exec flux-runtime: {}", err)
}

#[cfg(not(unix))]
async fn start_script(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    server_url: &str,
    token: &str,
    args: &RunArgs,
) -> Result<()> {
    let prog_args = build_runtime_args(server_url, token, args);

    let mut cmd = if let Some(bin) = binary {
        let mut c = tokio::process::Command::new(bin);
        c.args(&prog_args);
        c
    } else {
        let mut c = tokio::process::Command::new("cargo");
        c.current_dir(&workspace_root)
            .args(["run", "-p", "runtime", "--bin", "flux-runtime"]);
        if args.release {
            c.arg("--release");
        }
        c.arg("--").args(&prog_args);
        c
    };

    let status = cmd.spawn()
        .context("failed to spawn flux-runtime")?
        .wait()
        .await
        .context("flux-runtime exited unexpectedly")?;

    if !status.success() {
        bail!("flux-runtime exited with {}", status);
    }

    Ok(())
}

fn build_runtime_args(server_url: &str, token: &str, args: &RunArgs) -> Vec<String> {
    vec![
        "--entry".to_string(),
        args.entry.clone(),
        "--server-url".to_string(),
        server_url.to_string(),
        "--token".to_string(),
        token.to_string(),
        "--script-mode".to_string(),
    ]
}

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
        .find(|path| path.exists())
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
