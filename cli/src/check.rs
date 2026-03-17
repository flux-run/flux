use anyhow::{Result, bail};
use clap::Args;

use crate::project::{DiagnosticSeverity, analyze_project, has_errors, resolve_entry_path};

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,
}

pub async fn execute(args: CheckArgs) -> Result<()> {
    let entry = resolve_entry_path(args.entry.as_deref())?;
    let analysis = analyze_project(&entry).await?;

    println!("checked  {}", analysis.entry_path.display());
    println!("modules  {}", analysis.artifact.modules.len());
    println!("artifact {}", analysis.artifact.graph_sha256);

    for diagnostic in &analysis.diagnostics {
        let level = match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        };
        println!("{} [{}] {}: {}", level, diagnostic.code, diagnostic.specifier, diagnostic.message);
    }

    for npm in &analysis.npm_reports {
        println!("npm     {:?} {}: {}", npm.status, npm.specifier, npm.reason);
    }

    if has_errors(&analysis.diagnostics) {
        bail!("compatibility check failed")
    }

    Ok(())
}