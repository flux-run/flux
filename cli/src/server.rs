use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Start the Flux server.
    Start(ServerStartArgs),
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
    }
}

async fn execute_start(args: ServerStartArgs) -> Result<()> {
    let workspace_root = find_workspace_root()
        .ok_or_else(|| anyhow::anyhow!("could not locate workspace root containing Cargo.toml"))?;

    let database_url = args
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL must be set or passed with --database-url"))?;

    let service_token = args
        .service_token
        .or_else(|| std::env::var("INTERNAL_SERVICE_TOKEN").ok())
        .unwrap_or_else(|| "dev-service-token".to_string());

    let binary = find_server_binary(&workspace_root, args.release);
    if let Some(bin) = binary {
        println!("starting server binary {}", bin.display());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;

            let err = std::process::Command::new(&bin)
                .current_dir(&workspace_root)
                .env("GRPC_PORT", args.port.to_string())
                .env("DATABASE_URL", &database_url)
                .env("INTERNAL_SERVICE_TOKEN", &service_token)
                .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()))
                .exec();

            return Err(anyhow::anyhow!("failed to exec server binary: {}", err));
        }

        #[cfg(not(unix))]
        {
            let status = tokio::process::Command::new(&bin)
                .current_dir(&workspace_root)
                .env("GRPC_PORT", args.port.to_string())
                .env("DATABASE_URL", &database_url)
                .env("INTERNAL_SERVICE_TOKEN", &service_token)
                .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()))
                .status()
                .await
                .context("failed to start server binary")?;

            if !status.success() {
                bail!("server exited with {}", status);
            }
            return Ok(());
        }
    }

    println!("server binary not found, starting via cargo run");
    let mut command = tokio::process::Command::new("cargo");
    command.current_dir(&workspace_root);
    command.arg("run").arg("-p").arg("server");
    if args.release {
        command.arg("--release");
    }
    command.env("GRPC_PORT", args.port.to_string());
    command.env("DATABASE_URL", &database_url);
    command.env("INTERNAL_SERVICE_TOKEN", &service_token);
    command.env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()));

    let status = command.status().await.context("failed to start `cargo run -p server`")?;
    if !status.success() {
        bail!("server exited with {}", status);
    }

    Ok(())
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let contents = std::fs::read_to_string(&cargo_toml).ok()?;
            if contents.contains("[workspace]") {
                return Some(dir);
            }
        }

        if !dir.pop() {
            return None;
        }
    }
}

fn find_server_binary(workspace_root: &Path, release: bool) -> Option<PathBuf> {
    let name = if cfg!(windows) { "server.exe" } else { "server" };
    let primary = if release { "release" } else { "debug" };
    let secondary = if release { "debug" } else { "release" };

    [primary, secondary]
        .into_iter()
        .map(|profile| workspace_root.join("target").join(profile).join(name))
        .find(|path| path.exists())
}