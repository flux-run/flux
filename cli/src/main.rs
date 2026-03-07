use clap::{Parser, Subcommand};

mod api;
mod commands;

#[derive(Parser)]
#[command(name = "flux")]
#[command(about = "Fluxbase Control Plane Command Line Interface", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate the CLI with an API Key
    Login,
    /// Package and deploy the function in the current directory
    Deploy,
    // Future: Logs, Secrets
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Login => {
            commands::login::execute().await;
        }
        Commands::Deploy => {
            commands::deploy::execute().await;
        }
    }
}
