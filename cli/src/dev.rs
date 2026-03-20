use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

use crate::project::{resolve_entry_path, watch_fingerprint};

#[derive(Debug, Args)]
pub struct DevArgs {
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,

    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    #[arg(long, default_value_t = 1)]
    pub isolate_pool_size: usize,

    #[arg(long)]
    pub release: bool,

    #[arg(long, default_value_t = 500)]
    pub poll_ms: u64,

    #[arg(long)]
    pub watch_dir: Option<String>,
}

pub async fn execute(args: DevArgs) -> Result<()> {
    let entry = resolve_entry_path(args.entry.as_deref())?;
    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    let server_url = args
        .url
        .clone()
        .or_else(read_config_url)
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
    let token = args
        .token
        .clone()
        .or_else(read_config_token)
        .unwrap_or_default();

    let watch_dir = args
        .watch_dir
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| entry.parent().map(|path| path.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    eprintln!("flux dev  {}", entry.display());
    eprintln!("watching  {}", watch_dir.display());

    loop {
        // Build internal artifact for the runtime to resolve imports
        let analysis = crate::project::analyze_project(&entry)
            .await
            .context("failed to analyze project")?;
        
        if crate::project::has_errors(&analysis.diagnostics) {
            eprintln!("\nCompatibility Errors:");
            for diag in &analysis.diagnostics {
                if diag.severity == crate::project::DiagnosticSeverity::Error {
                    eprintln!("  ✘ [{}] {} {}", diag.code, diag.specifier, diag.message);
                }
            }
            eprintln!("\nFix errors to continue dev...");
        } else {
            let artifact_tmp = watch_dir.join(".flux_artifact_dev.json");
            crate::project::write_artifact(&artifact_tmp, &analysis.artifact)
                .context("failed to write dev artifact")?;

            let mut child = tokio::process::Command::new(&binary)
                .args(build_runtime_args(&artifact_tmp, &server_url, &token, &args))
                .spawn()
                .context("failed to spawn flux-runtime")?;
            eprintln!("[flux dev] started pid {:?}", child.id());

            let fingerprint_before = watch_fingerprint(&watch_dir)?;
            let should_restart = loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(args.poll_ms)).await;

                match child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!("[flux dev] runtime exited ({status}), restarting");
                        break true;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        eprintln!("[flux dev] wait error: {err}, restarting");
                        break true;
                    }
                }

                if watch_fingerprint(&watch_dir)? != fingerprint_before {
                    eprintln!("[flux dev] change detected, restarting");
                    break true;
                }
            };

            if should_restart {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }
}

fn build_runtime_args(artifact_path: &Path, server_url: &str, token: &str, args: &DevArgs) -> Vec<String> {
    vec![
        "--artifact".to_string(),
        artifact_path.to_string_lossy().into_owned(),
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

fn flux_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("config.toml")
}

fn read_config_url() -> Option<String> {
    let raw = std::fs::read_to_string(flux_config_path()).ok()?;
    raw.lines()
        .find(|line| line.starts_with("url"))
        .and_then(|line| line.splitn(2, '=').nth(1))
        .map(|value| value.trim().trim_matches('"').to_string())
}

fn read_config_token() -> Option<String> {
    let raw = std::fs::read_to_string(flux_config_path()).ok()?;
    raw.lines()
        .find(|line| line.starts_with("token"))
        .and_then(|line| line.splitn(2, '=').nth(1))
        .map(|value| value.trim().trim_matches('"').to_string())
}
