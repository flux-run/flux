use anyhow::{Result, Context};
use clap::Args;
use std::path::Path;

use crate::config::resolve_auth;

#[derive(Debug, Args)]
pub struct ReplayArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,

    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,

    #[arg(long)]
    pub commit: bool,
    #[arg(long)]
    pub validate: bool,
    #[arg(long)]
    pub explain: bool,
    #[arg(long, value_name = "PATHS", value_delimiter = ',')]
    pub ignore: Vec<String>,
    #[arg(long, value_name = "INDEX")]
    pub from_index: Option<i32>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub diff: bool,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    #[arg(long, default_value_t = 1)]
    pub isolate_pool_size: usize,

    #[arg(long)]
    pub release: bool,

    #[arg(long)]
    pub verbose: bool,
}

pub async fn execute(args: ReplayArgs) -> Result<()> {
    let entry = crate::project::resolve_entry_path(args.entry.as_deref())?;
    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    // Load .env from the project directory (silently ignore if missing)
    let project_dir = entry
        .parent()
        .and_then(|p| p.parent()) // go up from src/ to project root
        .unwrap_or_else(|| std::path::Path::new("."));
    let env_path = project_dir.join(".env");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    } else {
        // Also try current directory
        let _ = dotenvy::dotenv();
    }

    let auth = resolve_auth(args.url.clone(), args.token.clone())?;

    // Analyze project to create a temporary artifact
    let analysis = crate::project::analyze_project(&entry)
        .await
        .context("failed to analyze project")?;

    let artifact_tmp = std::env::temp_dir().join(format!(
        "flux_replay_{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    crate::project::write_artifact(&artifact_tmp, &analysis.artifact)
        .context("failed to write replay artifact")?;

    let runtime_args = build_runtime_args(&artifact_tmp, &auth.url, &auth.token, &args);

    println!("Replaying execution: {}", args.execution_id);

    let mut child = tokio::process::Command::new(&binary)
        .args(runtime_args)
        .spawn()
        .context("failed to spawn flux-runtime")?;

    let status = child.wait().await.context("failed to wait for flux-runtime")?;

    let dashboard_url = std::env::var("FLUX_DASHBOARD_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let project_id = analysis_project_id(&artifact_tmp).unwrap_or_else(|| "default".to_string());

    // Cleanup temp artifact
    let _ = std::fs::remove_file(artifact_tmp);

    println!("\n  {} Replay Finished\n", if status.success() { "✔" } else { "✘" });
    println!("  {} View in Dashboard:  {}/project/{}/executions/{}", "→", dashboard_url, project_id, args.execution_id);
    println!("  {} Debug root cause:   flux why {}\n", "→", args.execution_id);

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn build_runtime_args(
    artifact_path: &Path,
    server_url: &str,
    token: &str,
    args: &ReplayArgs,
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
        "--replay".to_string(),
        args.execution_id.clone(),
    ];

    if args.verbose {
        runtime_args.push("--verbose".to_string());
    }

    if args.release {
        runtime_args.push("--release".to_string());
    }

    if let Some(project_id) = analysis_project_id(artifact_path) {
        runtime_args.push("--project-id".to_string());
        runtime_args.push(project_id);
    }

    runtime_args
}

fn analysis_project_id(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    val.get("project_id")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}
