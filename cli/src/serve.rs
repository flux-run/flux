use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

use crate::config::resolve_optional_auth;
use crate::grpc::validate_service_token;
use crate::project::{resolve_built_artifact, resolve_entry_path};
use crate::runtime_process::{exec_runtime, find_runtime_binary, find_workspace_root};

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
    let auth = resolve_optional_auth(args.url.clone(), args.token.clone())?;
    if !args.skip_verify {
        validate_service_token(&auth.url, &auth.token).await?;
    }

    let workspace_root = find_workspace_root()
        .ok_or_else(|| anyhow::anyhow!("could not locate workspace root containing Cargo.toml"))?;

    let binary = find_runtime_binary(&workspace_root, args.release);

    write_runtime_port(args.port)?;
    write_runtime_entry(&entry.to_string_lossy())?;

    if !args.check_only {
        write_runtime_pid(std::process::id())?;
    }

    let prog_args = build_runtime_args(&auth.url, &auth.token, &built_artifact, &args);
    exec_runtime(workspace_root, binary, args.release, &prog_args).await
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