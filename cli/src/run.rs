use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::runtime_process::exec_runtime;
use crate::runtime_server::{RuntimeServerOptions, execute_server_runtime};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Entry file to execute as a plain script.
    #[arg(value_name = "ENTRY", default_value = "index.js")]
    pub entry: String,

    /// Keep the runtime alive as an HTTP listener instead of executing once.
    #[arg(long)]
    pub listen: bool,

    /// JSON input passed to the exported default handler, if present.
    /// Equivalent to the payload in `flux exec`. Ignored for top-level scripts.
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
}

pub async fn execute(args: RunArgs) -> Result<()> {
    if args.listen {
        return execute_server_runtime(RuntimeServerOptions {
            entry: Some(args.entry.clone()),
            url: args.url.clone(),
            token: args.token.clone(),
            skip_verify: args.skip_verify,
            host: args.host.clone(),
            port: args.port,
            isolate_pool_size: args.isolate_pool_size,
            check_only: args.check_only,
            release: args.release,
        })
        .await;
    }

    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    // Validate the input JSON eagerly so we give a clear error before spawning
    // the runtime process.
    let _: serde_json::Value = serde_json::from_str(&args.input)
        .with_context(|| format!("invalid --input JSON: {}", args.input))?;

    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    // Server URL and token are optional for script mode — default to empty
    // strings so the runtime can start without a running flux-server.
    let server_url = args
        .url
        .clone()
        .or_else(|| read_config_url())
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
    let token = args
        .token
        .clone()
        .or_else(|| read_config_token())
        .unwrap_or_default();

    let prog_args = build_runtime_args(&server_url, &token, &args);

    exec_runtime(binary, &prog_args).await
}

fn build_runtime_args(server_url: &str, token: &str, args: &RunArgs) -> Vec<String> {
    vec![
        "--entry".to_string(),
        args.entry.clone(),
        "--server-url".to_string(),
        server_url.to_string(),
        "--token".to_string(),
        token.to_string(),
        "--script-mode".to_string(),
        "--script-input".to_string(),
        args.input.clone(),
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
        .find(|l| l.starts_with("url"))
        .and_then(|l| l.splitn(2, '=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
}

fn read_config_token() -> Option<String> {
    let raw = std::fs::read_to_string(flux_config_path()).ok()?;
    raw.lines()
        .find(|l| l.starts_with("token"))
        .and_then(|l| l.splitn(2, '=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
}
