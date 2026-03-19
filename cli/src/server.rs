use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Start the Flux server.
    Start(ServerStartArgs),
    /// Restart the Flux server.
    Restart(ServerStartArgs),
}

#[derive(Debug, Args)]
pub struct ServerStartArgs {
    #[arg(long, default_value = "50051", value_name = "PORT")]
    pub port: u16,
    #[arg(long, env = "DATABASE_URL", value_name = "URL")]
    pub database_url: Option<String>,
    #[arg(long, env = "INTERNAL_SERVICE_TOKEN", value_name = "TOKEN")]
    pub service_token: Option<String>,
    #[arg(long)]
    pub release: bool,
}

pub async fn execute(command: ServerCommand) -> Result<()> {
    match command {
        ServerCommand::Start(args) => execute_start(args).await,
        ServerCommand::Restart(args) => execute_restart(args).await,
    }
}

async fn execute_restart(args: ServerStartArgs) -> Result<()> {
    stop_existing_server()?;
    execute_start(args).await
}

async fn execute_start(args: ServerStartArgs) -> Result<()> {
    let binary = crate::bin_resolution::ensure_binary("flux-server", args.release).await?;

    let database_url = args
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL must be set or passed with --database-url"))?;

    let service_token = args
        .service_token
        .or_else(|| std::env::var("INTERNAL_SERVICE_TOKEN").ok())
        .unwrap_or_else(|| "dev-service-token".to_string());

    write_server_port(args.port)?;

    println!("starting server binary {}", binary.display());
    start_server_binary(
        &binary,
        args.port,
        &database_url,
        &service_token,
    )
    .await
}

#[cfg(unix)]
async fn start_server_binary(
    bin: &Path,
    port: u16,
    database_url: &str,
    service_token: &str,
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    write_server_pid(std::process::id())?;

    let err = std::process::Command::new(bin)
        .env("GRPC_PORT", port.to_string())
        .env("DATABASE_URL", database_url)
        .env("INTERNAL_SERVICE_TOKEN", service_token)
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        )
        .exec();

    Err(anyhow::anyhow!("failed to exec server binary: {}", err))
}

#[cfg(not(unix))]
async fn start_server_binary(
    bin: &Path,
    port: u16,
    database_url: &str,
    service_token: &str,
) -> Result<()> {
    let status = tokio::process::Command::new(bin)
        .env("GRPC_PORT", port.to_string())
        .env("DATABASE_URL", database_url)
        .env("INTERNAL_SERVICE_TOKEN", service_token)
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        )
        .status()
        .await
        .context("failed to start server binary")?;

    if !status.success() {
        bail!("server exited with {}", status);
    }

    Ok(())
}

fn flux_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
}

fn server_pid_path() -> PathBuf {
    flux_dir().join("server.pid")
}

fn server_port_path() -> PathBuf {
    flux_dir().join("server.port")
}

fn write_server_pid(pid: u32) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(server_pid_path(), pid.to_string()).context("failed to write ~/.flux/server.pid")
}

fn write_server_port(port: u16) -> Result<()> {
    let dir = flux_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    std::fs::write(server_port_path(), port.to_string())
        .context("failed to write ~/.flux/server.port")
}

fn stop_existing_server() -> Result<()> {
    let pid_path = server_pid_path();
    if !pid_path.exists() {
        return Ok(());
    }

    let raw = std::fs::read_to_string(&pid_path).context("failed to read ~/.flux/server.pid")?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", pid_path.display()))?;

    if pid <= 0 {
        bail!("invalid server pid: {}", pid);
    }

    let status = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("failed to execute kill command")?;

    if !status.success() {
        bail!("failed to stop existing server process {}", pid);
    }

    let _ = std::fs::remove_file(pid_path);
    Ok(())
}
