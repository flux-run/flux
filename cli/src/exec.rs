use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::get_trace;

#[derive(Debug, Args)]
pub struct ExecArgs {
    #[arg(value_name = "ENTRY", default_value = "index.js")]
    pub entry: String,
    #[arg(long, value_name = "JSON", default_value = "{}")]
    pub input: String,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub release: bool,
    #[arg(long, default_value_t = 1)]
    pub isolate_pool_size: usize,
    #[arg(long, default_value_t = 10)]
    pub timeout_secs: u64,
}

pub async fn execute(args: ExecArgs) -> Result<()> {
    let auth = resolve_auth(args.url.clone(), args.token.clone())?;
    let payload_json: serde_json::Value =
        serde_json::from_str(&args.input).context("invalid --input JSON")?;

    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    let route_name = entry
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file stem: {}", entry.display()))?
        .to_string();

    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;

    let runtime_port = pick_free_port()?;

    let mut child = spawn_runtime(
        &binary,
        &args,
        &auth.url,
        &auth.token,
        runtime_port,
    )
    .await?;

    let result = run_one_off(
        runtime_port,
        &route_name,
        payload_json,
        args.timeout_secs,
        &auth.url,
        &auth.token,
    )
    .await;

    let _ = child.kill().await;
    let _ = child.wait().await;

    result
}

async fn run_one_off(
    runtime_port: u16,
    route_name: &str,
    payload_json: serde_json::Value,
    timeout_secs: u64,
    server_url: &str,
    token: &str,
) -> Result<()> {
    wait_for_runtime(runtime_port, timeout_secs).await?;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{}/{}", runtime_port, route_name))
        .json(&payload_json)
        .send()
        .await
        .context("failed to invoke runtime")?;

    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to decode runtime response JSON")?;

    let symbol = if status.is_success() {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[31m✗\x1b[0m"
    };
    println!();
    println!("  {}  exec {}", symbol, route_name);
    println!("  status  {}", status);
    println!("  output");
    print_json(&body, 4);

    let execution_id = body
        .get("execution_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if execution_id.is_empty() {
        println!();
        println!("  execution id not present in runtime response");
        println!();
        return Ok(());
    }

    let trace = get_trace(server_url, token, &execution_id).await?;
    println!();
    println!("  trace  {}", execution_id);
    println!(
        "  {} {}  {}  {}ms",
        trace.method, trace.path, trace.status, trace.duration_ms
    );
    if !trace.error.is_empty() {
        println!("  error  {}", trace.error);
    }
    println!();

    Ok(())
}

async fn spawn_runtime(
    binary: &Path,
    args: &ExecArgs,
    server_url: &str,
    token: &str,
    port: u16,
) -> Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--port")
        .arg(port.to_string())
        .arg("--isolate-pool-size")
        .arg(args.isolate_pool_size.to_string())
        .env("FLUX_SERVER_URL", server_url)
        .env("FLUX_SERVICE_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    command
        .spawn()
        .context("failed to start flux-runtime for one-off execution")
}

async fn wait_for_runtime(port: u16, timeout_secs: u64) -> Result<()> {
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        if std::time::Instant::now() > deadline {
            bail!("runtime did not become ready within {}s", timeout_secs);
        }

        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{}/health", port))
            .send()
            .await
        {
            if response.status().is_success() {
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

fn pick_free_port() -> Result<u16> {
    let listener =
        TcpListener::bind("127.0.0.1:0").context("failed to bind an ephemeral local port")?;
    let port = listener
        .local_addr()
        .context("failed to read local bound address")?
        .port();
    Ok(port)
}

fn print_json(value: &serde_json::Value, indent: usize) {
    let formatted = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    let prefix = " ".repeat(indent);
    for line in formatted.lines() {
        println!("{}{}", prefix, line);
    }
}
