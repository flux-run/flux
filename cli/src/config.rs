use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::fs;

// ─── flux.toml — project-level config (Flux v2 format) ─────────────────────
//
// Written by `flux init` at the project root.  Committed to version control.
//
// [dev] section is used by `flux dev` and the CLI URL resolver.
// [limits] section feeds function limits (combined with defineFunction / flux.json).
//
//   name    = "my-project"
//   runtime = "nodejs20"
//
//   [record]
//   sample_rate   = 1.0
//   retention_days = 30
//
//   [limits]
//   timeout_ms = 5000
//   memory_mb  = 256
//
//   [dev]
//   gateway_port     = 4000
//   runtime_port     = 8083
//   api_port         = 8080
//   data_engine_port = 8082
//   queue_port       = 8084

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FluxTomlRecord {
    pub sample_rate: Option<f64>,
    pub retention_days: Option<u32>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FluxTomlLimits {
    pub timeout_ms: Option<u64>,
    pub memory_mb: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FluxTomlDev {
    pub gateway_port: Option<u16>,
    pub runtime_port: Option<u16>,
    pub api_port: Option<u16>,
    pub data_engine_port: Option<u16>,
    pub queue_port: Option<u16>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FluxToml {
    pub name: Option<String>,
    pub runtime: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub record: FluxTomlRecord,
    #[serde(default)]
    pub limits: FluxTomlLimits,
    #[serde(default)]
    pub dev: FluxTomlDev,
}

impl FluxToml {
    const FILE: &'static str = "flux.toml";

    /// Walk from `cwd` toward the root looking for `flux.toml`.
    pub fn find_path() -> Option<PathBuf> {
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

    pub fn load_sync() -> Option<Self> {
        let path = Self::find_path()?;
        let src = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<Self>(&src) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                // Surface the error without aborting — callers treat None as
                // "no flux.toml" so we print a warning and fall through.
                eprintln!(
                    "warning: {} is malformed and was ignored ({})\n         \
                     Fix the file or delete it and re-run `flux init`.",
                    path.display(),
                    e,
                );
                None
            }
        }
    }

    /// Compute the API URL from [dev] ports (falls back to default).
    pub fn api_url(&self) -> Option<String> {
        self.dev.api_port.map(|p| format!("http://localhost:{}", p))
    }

    /// Compute the gateway URL from [dev] ports.
    pub fn gateway_url(&self) -> Option<String> {
        self.dev.gateway_port.map(|p| format!("http://localhost:{}", p))
    }

    /// Compute the runtime URL from [dev] ports.
    pub fn runtime_url(&self) -> Option<String> {
        self.dev.runtime_port.map(|p| format!("http://localhost:{}", p))
    }
}

// ─── CLI runtime config (~/.flux/config.json) ────────────────────────────────
//
// Loaded from `~/.flux/config.json`.  Override individual fields with:
//   FLUXBASE_API_URL   FLUXBASE_GATEWAY_URL   FLUXBASE_RUNTIME_URL
// `flux.toml [dev]` takes highest precedence for local port assignments.──

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
    /// Runtime URL — used by `flux invoke` to call the function execution engine.
    /// Env: FLUXBASE_RUNTIME_URL  Default: http://localhost:8083
    #[serde(default)]
    pub runtime_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url:     "http://localhost:8080".to_string(),
            token:       None,
            tenant_id:   None,
            tenant_slug: None,
            project_id:  None,
            gateway_url: "http://localhost:8081".to_string(),
            runtime_url: "http://localhost:8083".to_string(),
        }
    }
}

impl Config {
    fn config_path() -> PathBuf {
        let mut path = dirs::home_dir().expect("Could not find home directory");
        path.push(".flux");
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
        if let Ok(url) = std::env::var("FLUXBASE_RUNTIME_URL") {
            config.runtime_url = url;
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
            if let Some(url) = proj.runtime_url {
                config.runtime_url = url;
            }
        }

        // flux.toml [dev] ports override everything for local dev.
        // Precedence: flux.toml > env vars > .fluxbase/config.json > defaults.
        if let Some(flux) = FluxToml::load_sync() {
            if let Some(url) = flux.api_url() {
                config.api_url = url;
            }
            if let Some(url) = flux.gateway_url() {
                config.gateway_url = url;
            }
            if let Some(url) = flux.runtime_url() {
                config.runtime_url = url;
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
    /// Override the Fluxbase Runtime URL for this project (e.g. local dev instance).
    /// Used by `flux invoke` to call the function execution engine.
    pub runtime_url: Option<String>,
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
