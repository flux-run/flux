use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use tokio::io::AsyncBufReadExt;

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
    #[arg(long, value_name = "ID")]
    pub project_id: Option<String>,
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

    // Load .env from the project directory (silently ignore if missing).
    let entry_path = PathBuf::from(&args.entry);
    let env_path = entry_path
        .parent()
        .map(|p| p.join(".env"))
        .filter(|p| p.exists());
    if let Some(p) = env_path {
        let _ = dotenvy::from_path(&p);
        eprintln!("env       {}", p.display());
    } else if let Ok(p) = dotenvy::dotenv() {
        eprintln!("env       {}", p.display());
    }

    let project_id = args.project_id.clone().or_else(|| {
        let project_dir = entry.parent().unwrap_or(std::path::Path::new("."));
        crate::project::load_project_config(project_dir).ok().and_then(|c| c.project_id)
    });

    let runtime_port = pick_free_port()?;

    let mut child = spawn_runtime(
        &binary,
        &args,
        &auth.url,
        &auth.token,
        runtime_port,
        project_id.as_deref(),
    )
    .await?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut execution_id = String::new();

    // 1. Capture execution ID and stream output
    while let Ok(Some(line)) = reader.next_line().await {
        println!("{}", line);
        if let Some(pos) = line.find("[boot] execution_id=") {
            let rest = &line[pos + 20..];
            execution_id = rest.split_whitespace().next().unwrap_or_default().to_string();
            break;
        }
    }

    // Drain remaining stdout in background
    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            println!("{}", line);
        }
    });

    // 2. Wait for either the HTTP readiness OR for the process to exit
    let mut wait_for_ready = Box::pin(wait_for_runtime(runtime_port, args.timeout_secs));
    
    let result = tokio::select! {
        ready_res = &mut wait_for_ready => {
            if ready_res.is_ok() {
                // Runtime is ready as a server, call the handler
                let res = run_one_off(
                    runtime_port,
                    &route_name,
                    payload_json,
                    &auth.url,
                    &auth.token,
                ).await;
                let _ = child.kill().await;
                res
            } else {
                let _ = child.kill().await;
                ready_res
            }
        }
        exit_res = child.wait() => {
            // Runtime exited early (probably a script)
            if let Ok(_status) = exit_res {
                if !execution_id.is_empty() {
                    display_trace(&auth.url, &auth.token, &execution_id).await
                } else {
                    bail!("runtime exited without announcing an execution ID")
                }
            } else {
                bail!("runtime crashed")
            }
        }
    };

    result
}

async fn display_trace(server_url: &str, token: &str, execution_id: &str) -> Result<()> {
    let trace = get_trace(server_url, token, execution_id).await?;
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

async fn run_one_off(
    runtime_port: u16,
    route_name: &str,
    payload_json: serde_json::Value,
    server_url: &str,
    token: &str,
) -> Result<()> {
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

    if !execution_id.is_empty() {
        display_trace(server_url, token, &execution_id).await?;
    }

    Ok(())
}

async fn spawn_runtime(
    binary: &Path,
    args: &ExecArgs,
    server_url: &str,
    token: &str,
    port: u16,
    project_id: Option<&str>,
) -> Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--entry")
        .arg(&args.entry)
        .arg("--port")
        .arg(port.to_string())
        .arg("--isolate-pool-size")
        .arg(args.isolate_pool_size.to_string())
        .env("FLUX_SERVER_URL", server_url)
        .env("FLUX_SERVICE_TOKEN", token);

    if let Some(id) = project_id {
        command.arg("--project-id").arg(id);
    }

    command
        .stdout(Stdio::piped())
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
