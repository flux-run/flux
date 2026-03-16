use anyhow::Result;
use clap::{Subcommand, ValueEnum};

use crate::config::CliConfig;
use crate::grpc::normalize_grpc_url;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Set a config value.
    Set {
        #[arg(value_enum)]
        key: ConfigKey,
        value: String,
    },
    /// Get a config value or all values.
    Get {
        #[arg(value_enum)]
        key: Option<ConfigKey>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ConfigKey {
    Server,
    Token,
}

pub fn execute(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Set { key, value } => set(key, value),
        ConfigCommand::Get { key } => get(key),
    }
}

fn set(key: ConfigKey, value: String) -> Result<()> {
    let mut config = CliConfig::load()?;

    match key {
        ConfigKey::Server => config.url = Some(normalize_grpc_url(&value)),
        ConfigKey::Token => config.token = Some(value),
    }

    config.save()?;
    println!("saved config value");
    Ok(())
}

fn get(key: Option<ConfigKey>) -> Result<()> {
    let config = CliConfig::load()?;

    match key {
        Some(ConfigKey::Server) => {
            if let Some(url) = config.url {
                println!("{}", url);
            }
        }
        Some(ConfigKey::Token) => {
            if let Some(token) = config.token {
                println!("{}", token);
            }
        }
        None => {
            if let Some(url) = config.url {
                println!("server={}", url);
            }
            if let Some(token) = config.token {
                println!("token={}", token);
            }
        }
    }

    Ok(())
}
