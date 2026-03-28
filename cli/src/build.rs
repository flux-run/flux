use anyhow::{bail, Result};
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
            // Show a relative display path where possible.
            let display_spec = std::env::current_dir()
                .ok()
                .and_then(|cwd| {
                    let path = diagnostic
                        .specifier
                        .strip_prefix("file://")
                        .unwrap_or(&diagnostic.specifier);
                    std::path::Path::new(path)
                        .strip_prefix(&cwd)
                        .ok()
                        .map(|p| p.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| diagnostic.specifier.clone());

            eprintln!("error[{}]: {}", diagnostic.code, display_spec);
            // Strip the outer "failed to parse <url>: " wrapper that anyhow
            // adds — the filename is already shown on the line above.
            let msg = diagnostic
                .message
                .find(": ")
                .map(|i| diagnostic.message[i + 2..].trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(diagnostic.message.trim());
            eprintln!("  {}", msg);
        }
        bail!("build failed")
    }

    write_project_config(&analysis.project_dir, &analysis.config)?;
    write_artifact(&analysis.artifact_path, &analysis.artifact)?;

    println!("built    {}", analysis.entry_path.display());
    println!("artifact {}", analysis.artifact_path.display());
    println!("graph    {}", analysis.artifact.graph_sha256);
    println!("modules  {}", analysis.artifact.modules.len());

    Ok(())
}
