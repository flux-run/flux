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
        let legacy = legacy_config_path();
        if !path.exists() && !legacy.exists() {
            return Ok(Self::default());
        }

        let source_path = if path.exists() { path } else { legacy };

        let raw = std::fs::read_to_string(&source_path)
            .with_context(|| format!("failed to read {}", source_path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", source_path.display()))
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
        .or_else(load_server_url_from_port_file)
        .ok_or_else(|| anyhow::anyhow!("missing server URL\n\nrun:\n  flux init --auth"))?;

    let token = token
        .or(config.token)
        .ok_or_else(|| anyhow::anyhow!("missing service token\n\nrun:\n  flux init --auth"))?;

    Ok(ResolvedAuth {
        url: normalize_grpc_url(&url),
        token,
    })
}

pub fn resolve_optional_auth(url: Option<String>, token: Option<String>) -> Result<ResolvedAuth> {
    let config = CliConfig::load()?;

    let url = url
        .or(config.url)
        .or_else(load_server_url_from_port_file)
        .unwrap_or_else(|| "http://127.0.0.1:50051".to_string());

    let token = token
        .or(config.token)
        .unwrap_or_default();

    Ok(ResolvedAuth {
        url: normalize_grpc_url(&url),
        token,
    })
}

fn load_server_url_from_port_file() -> Option<String> {
    let path = dirs::home_dir()?.join(".flux").join("server.port");
    let raw = std::fs::read_to_string(path).ok()?;
    let port = raw.trim();
    if port.is_empty() {
        return None;
    }
    Some(format!("localhost:{}", port))
}

fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("config.toml")
}

fn legacy_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("cli.toml")
}
