use std::path::PathBuf;
use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Debug, Args)]
pub struct StartArgs {
    /// Path to a pre-built Flux artifact JSON (optional, defaults to .flux/artifact.json).
    #[arg(long, value_name = "FILE")]
    pub artifact: Option<String>,

    /// Specific version (SHA256) to run from the build history.
    #[arg(long, short, value_name = "SHA")]
    pub version: Option<String>,

    /// JSON input passed to the exported default handler.
    #[arg(long, value_name = "JSON", default_value = "{}")]
    pub input: String,

    /// Flux server URL for recording the execution (optional).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Service token for the Flux server (optional).
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    #[arg(long, default_value_t = 16)]
    pub isolate_pool_size: usize,
}

pub async fn execute(args: StartArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    
    // 1. Resolve artifact path
    let artifact_path = if let Some(ref path) = args.artifact {
        PathBuf::from(path)
    } else if let Some(ref sha) = args.version {
        // Look for the specific version in .flux/artifacts/
        let path = cwd.join(".flux").join("artifacts").join(format!("{}.json", sha));
        if !path.exists() {
            bail!("Version {} not found in .flux/artifacts/", sha);
        }
        path
    } else {
        // Look for .flux/artifact.json in the current directory
        cwd.join(".flux").join("artifact.json")
    };

    if !artifact_path.exists() {
        bail!(
            "No build artifact found at {}.\nRun `flux build` first to create one.",
            artifact_path.display()
        );
    }

    // 2. Resolve project ID from flux.json
    let project_config = crate::project::load_project_config(&cwd).ok();
    let project_id = project_config.as_ref().and_then(|c| c.project_id.clone());
    let project_kind = project_config.as_ref().map(|c| c.kind.clone()).unwrap_or(shared::project::ProjectKind::Function);

    // 3. Prepare runtime arguments
    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;
    let auth = crate::config::resolve_optional_auth(args.url.clone(), args.token.clone())?;

    let mut prog_args = vec![
        "--artifact".to_string(),
        artifact_path.to_string_lossy().to_string(),
        "--server-url".to_string(),
        auth.url.clone(),
        "--token".to_string(),
        auth.token.clone(),
        "--host".to_string(),
        args.host.clone(),
        "--port".to_string(),
        args.port.to_string(),
        "--isolate-pool-size".to_string(),
        args.isolate_pool_size.to_string(),
    ];

    if let Some(ref id) = project_id {
        prog_args.push("--project-id".to_string());
        prog_args.push(id.clone());
    }

    // If it's a function project, we usually want to serve it.
    if project_kind == shared::project::ProjectKind::Function {
        prog_args.push("--serve".to_string());
    }

    if !args.input.is_empty() && args.input != "{}" {
        prog_args.push("--script-input".to_string());
        prog_args.push(args.input.clone());
    }

    // 4. Run with unified runtime_runner
    let project_name = cwd.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("flux-project")
        .to_string();

    let project_id_copy = project_id.clone();
    crate::runtime_runner::run_with_tui(crate::runtime_runner::RuntimeConfig {
        project_name,
        project_id: project_id_copy,
        display_path: "artifact".to_string(),
        binary_path: binary,
        args: prog_args,
        server_url: auth.url.clone(),
        watch_dir: None,
        poll_ms: 0,
    }).await?;

    Ok(())
}
