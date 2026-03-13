use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::fs;

// ─── Default ports ────────────────────────────────────────────────────────────
//
// These are the ONLY place port numbers live.  Everything else — Config::default(),
// FluxToml URL helpers, dev.rs service table — reads from here.
// Override at runtime via flux.toml [dev] or FLUXBASE_*_URL env vars.

/// Legacy separate-binary port \u2014 kept for backward-compat with explicit `flux.toml [dev]` overrides.
#[allow(dead_code)]
pub const DEFAULT_API_PORT:         u16 = 8080;
#[allow(dead_code)]
pub const DEFAULT_GATEWAY_PORT:     u16 = 8081;
pub const DEFAULT_RUNTIME_PORT:     u16 = 8083;
pub const DEFAULT_DATA_ENGINE_PORT: u16 = 8082;
pub const DEFAULT_QUEUE_PORT:       u16 = 8084;
/// Monolithic server port — single binary serves all services on this port.
pub const DEFAULT_SERVER_PORT:      u16 = 4000;
/// Dev-only: Vite HMR port. In production the dashboard is served from the API
/// binary at `/ui/*` — no separate process or port needed.
pub const DEFAULT_DASHBOARD_PORT:   u16 = 5173;
pub const DEFAULT_DB_PORT:          u16 = 5432;

/// Build a localhost URL from a port number.
#[inline]
pub fn local_url(port: u16) -> String {
    format!("http://localhost:{}", port)
}

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
//   gateway_port     = 8081   # DEFAULT_GATEWAY_PORT
//   runtime_port     = 8083   # DEFAULT_RUNTIME_PORT
//   api_port         = 8080   # DEFAULT_API_PORT
//   data_engine_port = 8082   # DEFAULT_DATA_ENGINE_PORT
//   queue_port       = 8084   # DEFAULT_QUEUE_PORT
//   # dashboard_port not needed — dashboard is served by the API binary at /ui

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
    pub gateway_port:     Option<u16>,
    pub runtime_port:     Option<u16>,
    pub api_port:         Option<u16>,
    pub data_engine_port: Option<u16>,
    pub queue_port:       Option<u16>,
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
        self.dev.api_port.map(local_url)
    }

    /// Compute the gateway URL from [dev] ports.
    pub fn gateway_url(&self) -> Option<String> {
        self.dev.gateway_port.map(local_url)
    }

    /// Compute the runtime URL from [dev] ports.
    pub fn runtime_url(&self) -> Option<String> {
        self.dev.runtime_port.map(local_url)
    }

    /// Compute the data engine URL from [dev] ports.
    pub fn data_engine_url(&self) -> Option<String> {
        self.dev.data_engine_port.map(local_url)
    }

    /// Compute the queue URL from [dev] ports.
    pub fn queue_url(&self) -> Option<String> {
        self.dev.queue_port.map(local_url)
    }
}

// ─── .flux/config.json — per-project, single-binary mode ───────────────────
//
// Written by `flux init` in the project directory alongside `flux.toml`.
// Gitignored — contains the CLI key used to authenticate with the local server.
//
// Example .flux/config.json:
//
//   {
//     "server_url": "http://localhost:4000/flux/api",
//     "cli_key":    "dev-cli-key"
//   }
//
// URL precedence (highest → lowest):
//   FLUX_URL env var → .flux/config.json → FLUXBASE_API_URL → ~/.flux/config.json → default
//   FLUX_CLI_KEY env var → .flux/config.json → default (empty = no auth)

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FluxLocalConfig {
    /// Single-binary API base URL (e.g. http://localhost:4000/flux/api).
    pub server_url: Option<String>,
    /// Key sent as `Authorization: Bearer <cli_key>` to the Flux server.
    /// Must match the server's `FLUX_API_KEY` environment variable.
    pub cli_key: Option<String>,
}

impl FluxLocalConfig {
    const FILE: &'static str = ".flux/config.json";

    /// Walk from cwd toward the root looking for `.flux/config.json`.
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

    pub fn load_sync() -> Option<Self> {
        let path = Self::find_path()?;
        let src  = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&src).ok()
    }

    /// Write `.flux/config.json` in the current directory.
    pub async fn save(&self) -> Result<PathBuf, std::io::Error> {
        let path = PathBuf::from(Self::FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents).await?;
        Ok(path)
    }
}

//
// Loaded from `~/.flux/config.json`.  Override individual fields with:
//   FLUXBASE_API_URL   FLUXBASE_GATEWAY_URL   FLUXBASE_RUNTIME_URL
//   FLUXBASE_DATA_ENGINE_URL
// `flux.toml [dev]` takes highest precedence for local port assignments.
//
// No authentication fields — local services accept all traffic.
// The project context is the current directory: flux.toml is the project root.

/// Runtime configuration for the CLI.
///
/// Fields map to service URLs.  `cli_key` is sent as a bearer token so the
/// server can verify requests come from an authorised CLI instance.
///
/// **URL precedence** (highest → lowest):
///   1. `FLUX_URL` env var          → api_url
///   2. `.flux/config.json`  (cwd)  → server_url + cli_key
///   3. `FLUXBASE_API_URL` env var  → api_url
///   4. `~/.flux/config.json`       → stored config
///   5. Compiled-in defaults        → http://localhost:4000/flux/api
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Flux server API base — `http://localhost:4000/flux/api` (single binary)
    pub api_url: String,
    /// Local gateway — `http://localhost:4000`
    #[serde(default)]
    pub gateway_url: String,
    /// Local runtime — `http://localhost:8083` (legacy separate-binary mode)
    #[serde(default)]
    pub runtime_url: String,
    /// Local data engine — `http://localhost:8082` (legacy)
    #[serde(default)]
    pub data_engine_url: String,
    /// Local queue — `http://localhost:8084` (legacy)
    #[serde(default)]
    pub queue_url: String,
    /// Bearer token for CLI → server authentication.
    /// Set to the server's `FLUX_API_KEY` value.  Empty = no auth (dev mode).
    #[serde(default)]
    pub cli_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Single-binary server — all APIs served at :4000/flux/api.
            api_url:         format!("http://localhost:{}/flux/api", DEFAULT_SERVER_PORT),
            gateway_url:     local_url(DEFAULT_SERVER_PORT),
            runtime_url:     local_url(DEFAULT_RUNTIME_PORT),
            data_engine_url: local_url(DEFAULT_DATA_ENGINE_PORT),
            queue_url:       local_url(DEFAULT_QUEUE_PORT),
            cli_key:         None,
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

        // ── .flux/config.json (project-local, single-binary mode) ────────
        // Highest priority for URL + key — overrides everything below.
        if let Some(local) = FluxLocalConfig::load_sync() {
            if let Some(url) = local.server_url {
                config.api_url = url;
            }
            if local.cli_key.is_some() {
                config.cli_key = local.cli_key;
            }
        }

        // ── FLUX_URL / FLUX_CLI_KEY override .flux/config.json ───────────
        if let Ok(url) = std::env::var("FLUX_URL") {
            config.api_url = url;
        }
        if let Ok(key) = std::env::var("FLUX_CLI_KEY") {
            config.cli_key = Some(key);
        }

        // ── Legacy env vars ───────────────────────────────────────────────
        if let Ok(url) = std::env::var("FLUXBASE_API_URL") {
            config.api_url = url;
        }
        if let Ok(url) = std::env::var("FLUXBASE_GATEWAY_URL") {
            config.gateway_url = url;
        }
        if let Ok(url) = std::env::var("FLUXBASE_RUNTIME_URL") {
            config.runtime_url = url;
        }
        if let Ok(url) = std::env::var("FLUXBASE_DATA_ENGINE_URL") {
            config.data_engine_url = url;
        }
        if let Ok(url) = std::env::var("FLUXBASE_QUEUE_URL") {
            config.queue_url = url;
        }

        // flux.toml [dev] port overrides take highest precedence.
        // They are committed to version control so the whole team uses the
        // same ports without any extra flags.
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
            if let Some(url) = flux.data_engine_url() {
                config.data_engine_url = url;
            }
            if let Some(url) = flux.queue_url() {
                config.queue_url = url;
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
