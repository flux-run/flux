use crate::config;
use crate::grpc;
use anyhow::{Context, Result};
use clap::Args;
use shared::project::{FluxBuildArtifact, FluxProjectConfig};
use std::path::Path;

#[derive(Args)]
pub struct DeployArgs {
    /// Function name to deploy (optional, defaults to project kind match)
    #[arg(long)]
    pub name: Option<String>,

    /// Project ID to deploy to (optional, defaults to config)
    #[arg(long)]
    pub project_id: Option<String>,
}

pub async fn execute(args: DeployArgs) -> Result<()> {
    // 1. Load project config
    let config_path = Path::new("flux.json");
    if !config_path.exists() {
        anyhow::bail!("No flux.json found. Run 'flux init' first.");
    }
    let config_file = std::fs::read_to_string(config_path)?;
    let project_config: FluxProjectConfig = serde_json::from_str(&config_file)?;

    // 2. Perform build
    println!("📦 Building project...");
    crate::build::execute(crate::build::BuildArgs { entry: None }).await?;

    // 3. Load artifact
    let artifact_path = Path::new(&project_config.artifact);
    if !artifact_path.exists() {
        anyhow::bail!(
            "Build failed: artifact not found at {}",
            project_config.artifact
        );
    }
    let artifact_json = std::fs::read_to_string(artifact_path)?;
    let _artifact: FluxBuildArtifact = serde_json::from_str(&artifact_json)?;

    // 4. Resolve auth and project context
    let auth = config::resolve_optional_auth(None, None)?;
    let project_id = args
        .project_id
        .or(project_config.project_id)
        .or(auth.project_id)
        .context("No project_id found. Please specify --project-id or log in to a project.")?;

    let function_name = args.name.unwrap_or_else(|| {
        // Default to a sanitized version of the current directory if not specified
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "default".to_string())
    });

    println!(
        "🚀 Deploying function '{}' to project {}...",
        function_name, project_id
    );

    // 5. Trigger deployment RPC
    let response = grpc::deploy_function(
        &auth.url,
        &auth.token,
        &project_id,
        &function_name,
        &artifact_json,
    )
    .await?;

    if response.ok {
        println!("✅ Deployment successful!");
        println!("   Function ID: {}", response.function_id);
        println!("   {}", response.message);
        println!("\nYour function is now live on Flux Cloud.");
    } else {
        anyhow::bail!("Deployment failed: {}", response.message);
    }

    Ok(())
}
