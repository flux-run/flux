use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::fs;

// ─── Global auth config (~/.fluxbase/config.json) ────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_url: String,
    pub token: Option<String>,
    pub tenant_id: Option<String>,
    pub tenant_slug: Option<String>,
    pub project_id: Option<String>,
    /// Gateway URL — used by `flux subscribe` / SDK realtime features.
    #[serde(default)]
    pub gateway_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "https://api.fluxbase.co".to_string(),
            token: None,
            tenant_id: None,
            tenant_slug: None,
            project_id: None,
            gateway_url: "https://gateway.fluxbase.co".to_string(),
        }
    }
}

impl Config {
    fn config_path() -> PathBuf {
        let mut path = dirs::home_dir().expect("Could not find home directory");
        path.push(".fluxbase");
        path.push("config.json");
        path
    }

    pub async fn load() -> Self {
        let path = Self::config_path();

        let mut config = if path.exists() {
            let contents = fs::read_to_string(&path).await.unwrap_or_default();
            serde_json::from_str(&contents).unwrap_or_else(|_| Config::default())
        } else {
            Config::default()
        };

        // Env vars take priority over the stored file.
        if let Ok(url) = std::env::var("FLUXBASE_API_URL") {
            config.api_url = url;
        }
        if let Ok(url) = std::env::var("FLUXBASE_GATEWAY_URL") {
            config.gateway_url = url;
        }
        if let Ok(v) = std::env::var("FLUXBASE_PROJECT_ID") {
            config.project_id = Some(v);
        }
        if let Ok(v) = std::env::var("FLUXBASE_TENANT_ID") {
            config.tenant_id = Some(v);
        }

        // Per-project config (.fluxbase/config.json) overrides env vars for
        // project-scoped settings so local dev instances are easy to wire up.
        if let Some(proj) = ProjectConfig::load_sync() {
            if proj.project_id.is_some() {
                config.project_id = proj.project_id;
            }
            if let Some(url) = proj.api_url {
                config.api_url = url;
            }
            if let Some(url) = proj.gateway_url {
                config.gateway_url = url;
            }
        }

        config
    }

    pub async fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(path, contents).await?;
        Ok(())
    }
}

// ─── Per-project config (.fluxbase/config.json in cwd) ───────────────────────
//
// Developers commit this file to version control so the whole team uses the
// same project ID and SDK output path without needing extra CLI flags.
//
// Example .fluxbase/config.json:
//
//   {
//     "project_id":     "proj_abc123",
//     "sdk_output":     "src/fluxbase.generated.ts",
//     "watch_interval": 5
//   }

/// Per-project settings read from `.fluxbase/config.json` in the current
/// working directory (or any parent directory, walking up like git does).
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ProjectConfig {
    /// Fluxbase project ID — overrides the global config when present.
    pub project_id: Option<String>,
    /// Default output path for `flux pull` / `flux watch` / `flux status`.
    pub sdk_output: Option<String>,
    /// Default polling interval (seconds) for `flux watch`.
    pub watch_interval: Option<u64>,
    /// Override the Fluxbase API URL for this project (e.g. local dev instance).
    /// Takes precedence over `FLUXBASE_API_URL` env var and global config.
    pub api_url: Option<String>,
    /// Override the Fluxbase Gateway URL for this project (e.g. local dev instance).
    /// Used by SDK `subscribe()` for SSE streams.
    pub gateway_url: Option<String>,
}

impl ProjectConfig {
    const FILE: &'static str = ".fluxbase/config.json";

    /// Walk from `cwd` toward the root looking for `.fluxbase/config.json`,
    /// exactly like git finds `.git/`.  Returns the first file found.
    fn find_path() -> Option<PathBuf> {
        let mut dir = std::env::current_dir().ok()?;
        loop {
            let candidate = dir.join(Self::FILE);
            if candidate.exists() {
                return Some(candidate);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// Public version of `find_path` for use in other modules (e.g. doctor).
    pub fn find_path_pub() -> Option<PathBuf> {
        Self::find_path()
    }

    /// Synchronous loader used inside `Config::load()` (called from an async
    /// context where we don't want an extra `.await`).
    pub fn load_sync() -> Option<Self> {
        let path = Self::find_path()?;
        let src  = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&src).ok()
    }

    /// Async loader for use in commands that need all fields.
    pub async fn load() -> Option<Self> {
        let path = Self::find_path()?;
        let src  = fs::read_to_string(path).await.ok()?;
        serde_json::from_str(&src).ok()
    }

    /// Return the path where the project config would be written.
    pub fn default_path() -> PathBuf {
        PathBuf::from(Self::FILE)
    }

    /// Persist the project config to `.fluxbase/config.json` in cwd.
    pub async fn save(&self) -> Result<PathBuf, std::io::Error> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents).await?;
        Ok(path)
    }

    // ── Helpers for sdk.rs  ──────────────────────────────────────────────

    /// Effective SDK output path: flag value → project config → default.
    pub fn resolve_sdk_output(flag: Option<String>, proj: Option<&ProjectConfig>) -> String {
        flag
            .or_else(|| proj.and_then(|p| p.sdk_output.clone()))
            .unwrap_or_else(|| "fluxbase.generated.ts".into())
    }

    /// Effective watch interval: flag value → project config → 5s.
    pub fn resolve_watch_interval(flag: u64, proj: Option<&ProjectConfig>) -> u64 {
        if flag != 5 { return flag; } // non-default flag wins
        proj.and_then(|p| p.watch_interval).unwrap_or(5)
    }
}
