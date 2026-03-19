use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::resolve_optional_auth;
use crate::grpc::validate_service_token;
use crate::project::{resolve_built_artifact, resolve_entry_path};
use crate::runtime_process::exec_runtime;

#[derive(Debug, Clone)]
pub struct RuntimeServerOptions {
    pub entry: Option<String>,
    pub url: Option<String>,
    pub token: Option<String>,
    pub skip_verify: bool,
    pub host: String,
    pub port: u16,
    pub isolate_pool_size: usize,
    pub check_only: bool,
    pub release: bool,
}

pub async fn execute_server_runtime(options: RuntimeServerOptions) -> Result<()> {
    let entry = resolve_entry_path(options.entry.as_deref())?;
    let (_config, built_artifact) = resolve_built_artifact(&entry)?;
    let auth = resolve_optional_auth(options.url.clone(), options.token.clone())?;
    if !options.skip_verify {
        validate_service_token(&auth.url, &auth.token).await?;
    }

    let binary = crate::bin_resolution::ensure_binary("flux-runtime", options.release).await?;

    write_runtime_port(options.port)?;
    write_runtime_entry(&entry.to_string_lossy())?;
    if !options.check_only {
        write_runtime_pid(std::process::id())?;
    }

    let prog_args = build_runtime_args(&auth.url, &auth.token, &built_artifact, &options);
    exec_runtime(binary, &prog_args).await
}

fn build_runtime_args(
    server_url: &str,
    token: &str,
    built_artifact: &Path,
    options: &RuntimeServerOptions,
) -> Vec<String> {
    let mut args = vec![
        "--artifact".to_string(),
        built_artifact.to_string_lossy().into_owned(),
        "--server-url".to_string(),
        server_url.to_string(),
        "--token".to_string(),
        token.to_string(),
        "--host".to_string(),
        options.host.clone(),
        "--port".to_string(),
        options.port.to_string(),
        "--isolate-pool-size".to_string(),
        options.isolate_pool_size.to_string(),
    ];

    if options.check_only {
        args.push("--check-only".to_string());
    }

    args
}

fn flux_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
}

fn write_runtime_pid(pid: u32) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.pid"), pid.to_string())
        .context("failed to write ~/.flux/runtime.pid")
}

fn write_runtime_port(port: u16) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.port"), port.to_string())
        .context("failed to write ~/.flux/runtime.port")
}

fn write_runtime_entry(entry: &str) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(dir.join("runtime.entry"), entry)
        .context("failed to write ~/.flux/runtime.entry")
}
