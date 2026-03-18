use anyhow::{Result, bail};
use clap::Args;

use crate::project::{
    analyze_project, has_errors, resolve_entry_path, write_artifact, write_project_config,
};

#[derive(Debug, Args)]
pub struct BuildArgs {
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,
}

pub async fn execute(args: BuildArgs) -> Result<()> {
    let entry = resolve_entry_path(args.entry.as_deref())?;
    let analysis = analyze_project(&entry).await?;

    if has_errors(&analysis.diagnostics) {
        for diagnostic in &analysis.diagnostics {
            println!(
                "error   [{}] {}: {}",
                diagnostic.code, diagnostic.specifier, diagnostic.message
            );
        }
        bail!("build failed due to unsupported imports or syntax")
    }

    write_project_config(&analysis.project_dir, &analysis.config)?;
    write_artifact(&analysis.artifact_path, &analysis.artifact)?;

    println!("built    {}", analysis.entry_path.display());
    println!("artifact {}", analysis.artifact_path.display());
    println!("graph    {}", analysis.artifact.graph_sha256);
    println!("modules  {}", analysis.artifact.modules.len());

    Ok(())
}
