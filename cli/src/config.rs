use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::grpc::normalize_grpc_url;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliConfig {
    pub url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub url: String,
    pub token: String,
}

impl CliConfig {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = toml::to_string_pretty(self).context("failed to serialize CLI config")?;
        std::fs::write(&path, raw)
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

pub fn resolve_auth(url: Option<String>, token: Option<String>) -> Result<ResolvedAuth> {
    let config = CliConfig::load()?;

    let url = url
        .or(config.url)
        .ok_or_else(|| anyhow::anyhow!("missing server URL: run `flux auth --url <host:port>` first"))?;

    let token = token
        .or(config.token)
        .ok_or_else(|| anyhow::anyhow!("missing service token: run `flux auth` first or pass --token"))?;

    Ok(ResolvedAuth {
        url: normalize_grpc_url(&url),
        token,
    })
}

fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("cli.toml")
}
