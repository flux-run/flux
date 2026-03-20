use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::runtime_process::exec_runtime;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Entry file to execute as a plain script.
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,

    /// Path to a pre-built Flux artifact JSON.
    #[arg(long, value_name = "FILE")]
    pub artifact: Option<String>,

    /// JSON input passed to the exported default handler, if present.
    /// Equivalent to the payload in `flux exec`. Ignored for top-level scripts.
    #[arg(long, value_name = "JSON", default_value = "{}")]
    pub input: String,

    /// Keep the runtime alive as an HTTP listener if a default handler is exported.
    /// (Automatically enabled if Deno.serve() is called).
    #[arg(long)]
    pub serve: bool,

    /// Flux server URL for recording the execution (optional).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Service token for the Flux server (optional).
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,

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

    /// Project ID for this execution.
    #[arg(long, value_name = "ID")]
    pub project_id: Option<String>,
}

pub async fn execute(args: RunArgs) -> Result<()> {
    if args.entry.is_none() && args.artifact.is_none() {
        bail!("either ENTRY or --artifact <FILE> must be provided");
    }

    if let Some(ref entry_str) = args.entry {
        let entry = PathBuf::from(entry_str);
        if !entry.exists() {
            bail!("entry file not found: {}", entry.display());
        }
    }

    if let Some(ref artifact_str) = args.artifact {
        let artifact = PathBuf::from(artifact_str);
        if !artifact.exists() {
            bail!("artifact file not found: {}", artifact.display());
        }
    }

    // Validate the input JSON eagerly so we give a clear error before spawning
    // the runtime process.
    let _: serde_json::Value = serde_json::from_str(&args.input)
        .with_context(|| format!("invalid --input JSON: {}", args.input))?;

    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    let auth = crate::config::resolve_optional_auth(args.url.clone(), args.token.clone())?;

    let project_id = args.project_id.clone().or_else(|| {
        if let Some(ref entry_str) = args.entry {
            let entry = std::path::PathBuf::from(entry_str);
            let project_dir = entry.parent().unwrap_or(std::path::Path::new("."));
            crate::project::load_project_config(project_dir).ok().and_then(|c| c.project_id)
        } else {
            None
        }
    });

    let prog_args = build_runtime_args(&auth.url, &auth.token, &args, project_id.as_deref());

    exec_runtime(binary, &prog_args).await
}

fn build_runtime_args(server_url: &str, token: &str, args: &RunArgs, project_id: Option<&str>) -> Vec<String> {
    let mut prog_args = Vec::new();

    if let Some(ref artifact) = args.artifact {
        prog_args.push("--artifact".to_string());
        prog_args.push(artifact.clone());
    } else if let Some(ref entry) = args.entry {
        prog_args.push("--entry".to_string());
        prog_args.push(entry.clone());
    }

    prog_args.extend(vec![
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
    ]);

    if let Some(project_id) = project_id {
        prog_args.push("--project-id".to_string());
        prog_args.push(project_id.to_string());
    }

    if args.serve {
        prog_args.push("--serve".to_string());
    }

    if !args.input.is_empty() && args.input != "{}" {
        prog_args.push("--script-input".to_string());
        prog_args.push(args.input.clone());
    }

    if args.check_only {
        prog_args.push("--check-only".to_string());
    }

    prog_args
}
