use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

use crate::project::resolve_entry_path;

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

    /// Keep the runtime alive as an HTTP listener if a default handler is exported.
    #[arg(long, alias = "listen")]
    pub serve: bool,
}

pub async fn execute(args: DevArgs) -> Result<()> {
    let entry = resolve_entry_path(args.entry.as_deref())?;
    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    // Load .env from the project directory (silently ignore if missing)
    let project_dir = entry
        .parent()
        .and_then(|p| p.parent()) // go up from src/ to project root
        .unwrap_or_else(|| std::path::Path::new("."));
    let env_path = project_dir.join(".env");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
        eprintln!("env       {}", env_path.display());
    } else {
        // Also try current directory
        let _ = dotenvy::dotenv();
    }

    let auth = crate::config::resolve_optional_auth(args.url.clone(), args.token.clone())?;

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
            tokio::time::sleep(tokio::time::Duration::from_millis(args.poll_ms)).await;
            continue;
        }

        let artifact_tmp = watch_dir.join(".flux_artifact_dev.json");
        crate::project::write_artifact(&artifact_tmp, &analysis.artifact)
            .context("failed to write dev artifact")?;

        let project_id = analysis.artifact.project_id.clone();
        let is_function = analysis.config.kind == shared::project::ProjectKind::Function;
        
        let runtime_args = build_runtime_args(
            &artifact_tmp,
            &auth.url,
            &auth.token,
            &args,
            project_id.as_deref(),
            is_function,
        );

        let result = crate::runtime_runner::run_with_tui(crate::runtime_runner::RuntimeConfig {
            project_name: "flux-dev".to_string(),
            project_id: project_id.clone(),
            display_path: entry.to_string_lossy().to_string(),
            binary_path: binary.clone(),
            args: runtime_args,
            server_url: auth.url.clone(),
            watch_dir: Some(watch_dir.clone()),
            poll_ms: args.poll_ms,
        }).await?;

        if result == crate::runtime_runner::RunResult::Finished {
            break;
        }

        // Small delay before restarting after change detection
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    Ok(())
}

fn build_runtime_args(
    artifact_path: &Path,
    server_url: &str,
    token: &str,
    args: &DevArgs,
    project_id: Option<&str>,
    is_function: bool,
) -> Vec<String> {
    let mut runtime_args = vec![
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
    ];

    if let Some(project_id) = project_id {
        runtime_args.push("--project-id".to_string());
        runtime_args.push(project_id.to_string());
    }

    if args.serve || is_function {
        runtime_args.push("--serve".to_string());
    }

    runtime_args
}
