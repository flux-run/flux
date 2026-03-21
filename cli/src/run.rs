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

    /// Execution ID to replay using local code.
    #[arg(long, value_name = "ID")]
    pub replay: Option<String>,
}

pub async fn execute(args: RunArgs) -> Result<()> {
    if args.entry.is_none() && args.artifact.is_none() {
        bail!("either ENTRY or --artifact <FILE> must be provided");
    }

    let mut temp_entry = None;
    if let Some(ref entry_str) = args.entry {
        if entry_str == "-" {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer).context("failed to read from stdin")?;
            
            let cwd = std::env::current_dir().context("failed to get current directory")?;
            let file_path = cwd.join(format!(".flux-stdin-{}.ts", uuid::Uuid::new_v4()));
            std::fs::write(&file_path, buffer).context("failed to write temp stdin file")?;
            temp_entry = Some(file_path.to_string_lossy().to_string());
        } else {
            let entry = PathBuf::from(entry_str);
            if !entry.exists() {
                bail!("entry file not found: {}", entry.display());
            }
        }
    }
    let entry_str = temp_entry.as_ref().or(args.entry.as_ref());

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

    // Load .env from the project directory (silently ignore if missing).
    // Try the artifact/entry directory first, then the current working directory.
    let loaded_env = if let Some(ref artifact_str) = args.artifact {
        let artifact = PathBuf::from(artifact_str);
        // artifact is typically at <project>/<src>/.flux/artifact.json — go up 3 levels
        let env_path = artifact
            .parent()  // .flux/
            .and_then(|p| p.parent())  // src/
            .and_then(|p| p.parent())  // project root
            .map(|p| p.join(".env"))
            .filter(|p| p.exists());
        if let Some(p) = env_path {
            let _ = dotenvy::from_path(&p);
            Some(p)
        } else {
            dotenvy::dotenv().ok().map(PathBuf::from)
        }
    } else if let Some(ref entry_str) = entry_str {
        let entry = PathBuf::from(entry_str);
        let env_path = entry
            .parent()
            .map(|p| p.join(".env"))
            .filter(|p| p.exists());
        if let Some(p) = env_path {
            let _ = dotenvy::from_path(&p);
            Some(p)
        } else {
            dotenvy::dotenv().ok().map(PathBuf::from)
        }
    } else {
        dotenvy::dotenv().ok().map(PathBuf::from)
    };
    if let Some(ref p) = loaded_env {
        eprintln!("env       {}", p.display());
    }

    let auth = crate::config::resolve_optional_auth(args.url.clone(), args.token.clone())?;

    let project_id = args.project_id.clone().or_else(|| {
        if let Some(ref entry_str) = entry_str {
            let entry = std::path::PathBuf::from(entry_str);
            let project_dir = entry.parent().unwrap_or(std::path::Path::new("."));
            crate::project::load_project_config(project_dir).ok().and_then(|c| c.project_id)
        } else {
            None
        }
    });

    let prog_args = build_runtime_args(&auth.url, &auth.token, &args, entry_str.map(|s| s.as_str()), project_id.as_deref());

    let res = exec_runtime(binary, &prog_args).await;
    
    if let Some(path_str) = temp_entry {
        let _ = std::fs::remove_file(path_str);
    }

    res
}

fn build_runtime_args(server_url: &str, token: &str, args: &RunArgs, entry_str: Option<&str>, project_id: Option<&str>) -> Vec<String> {
    let mut prog_args = Vec::new();

    if let Some(ref artifact) = args.artifact {
        prog_args.push("--artifact".to_string());
        prog_args.push(artifact.clone());
    } else if let Some(entry_str) = entry_str {
        prog_args.push("--entry".to_string());
        prog_args.push(entry_str.to_string());
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

    if let Some(ref replay_id) = args.replay {
        prog_args.push("--replay".to_string());
        prog_args.push(replay_id.clone());
    }

    prog_args
}
