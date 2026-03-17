use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::validate_service_token;
use crate::project::{resolve_built_artifact, resolve_entry_path};

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub skip_verify: bool,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
    #[arg(long, default_value_t = 16)]
    pub isolate_pool_size: usize,
    #[arg(long)]
    pub check_only: bool,
    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,
}

pub async fn execute(args: ServeArgs) -> Result<()> {
    let entry = resolve_entry_path(args.entry.as_deref())?;
    let (_config, built_artifact) = resolve_built_artifact(&entry)?;
    let auth = resolve_auth(args.url.clone(), args.token.clone())?;
    if !args.skip_verify {
        validate_service_token(&auth.url, &auth.token).await?;
    }

    let workspace_root = find_workspace_root()
        .ok_or_else(|| anyhow::anyhow!("could not locate workspace root containing Cargo.toml"))?;

    let binary = find_runtime_binary(&workspace_root, args.release);

    write_runtime_port(args.port)?;
    write_runtime_entry(&entry.to_string_lossy())?;

    start_runtime(workspace_root, binary, &auth.url, &auth.token, &built_artifact, &args).await
}

#[cfg(unix)]
async fn start_runtime(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    server_url: &str,
    token: &str,
    built_artifact: &Path,
    args: &ServeArgs,
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    if !args.check_only {
        write_runtime_pid(std::process::id())?;
    }

    let prog_args = build_runtime_args(server_url, token, built_artifact, args);

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
async fn start_runtime(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    server_url: &str,
    token: &str,
    built_artifact: &Path,
    args: &ServeArgs,
) -> Result<()> {
    let prog_args = build_runtime_args(server_url, token, built_artifact, args);

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

    let mut child = cmd.spawn().context("failed to spawn flux-runtime")?;
    if !args.check_only {
        if let Some(pid) = child.id() {
        write_runtime_pid(pid)?;
        }
    }

    let status = child.wait().await.context("flux-runtime exited unexpectedly")?;
    if !status.success() {
        bail!("flux-runtime exited with {}", status);
    }

    Ok(())
}

fn build_runtime_args(server_url: &str, token: &str, built_artifact: &Path, args: &ServeArgs) -> Vec<String> {
    let mut v = vec![
        "--artifact".to_string(),
        built_artifact.to_string_lossy().into_owned(),
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
    ];
    if args.check_only {
        v.push("--check-only".to_string());
    }
    v
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

fn flux_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
}

fn write_runtime_pid(pid: u32) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.pid"), pid.to_string())
        .context("failed to write ~/.flux/runtime.pid")
}

fn write_runtime_port(port: u16) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.port"), port.to_string())
        .context("failed to write ~/.flux/runtime.port")
}

fn write_runtime_entry(entry: &str) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.entry"), entry)
        .context("failed to write ~/.flux/runtime.entry")
}