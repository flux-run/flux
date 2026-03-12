//! HTTP transport for the Flux CLI.
//!
//! `ApiClient` is a thin wrapper around `reqwest::Client` pre-wired to the
//! URLs of the local Flux services.  No authentication is required — local
//! services accept all traffic.
//!
//! # URL resolution (highest precedence first)
//! 1. `flux.toml [dev]` port overrides
//! 2. `FLUXBASE_*_URL` environment variables
//! 3. Hard-coded localhost defaults (api :8080, gateway :4000, etc.)

use std::time::Duration;

use reqwest::Client;

use crate::config::Config;

// ─── ApiClient ────────────────────────────────────────────────────────────────

/// HTTP client pre-wired to the local Flux services.
///
/// Responsibilities (Single Responsibility Principle):
/// - Hold a single `reqwest::Client` (connection-pool reuse)
/// - Expose the resolved service base URLs
/// - Provide lightweight async helpers for common API calls
///
/// This type intentionally does NOT:
/// - Perform any authentication
/// - Know about tenants or projects
/// - Assume a remote endpoint
pub struct ApiClient {
    /// Underlying reqwest connection pool.
    pub client: Client,
    /// Local API service — `http://localhost:8080`
    pub base_url: String,
    /// Local gateway — `http://localhost:8081`
    pub gateway_url: String,
    /// Local runtime — `http://localhost:8083`
    pub runtime_url: String,
    /// Local data engine — `http://localhost:8082`
    pub data_engine_url: String,
    /// Local queue — `http://localhost:8084`
    pub queue_url: String,
    // dashboard_url intentionally absent — the dashboard is served from the
    // API binary itself at /ui, so the CLI never needs to address it directly.
}

impl ApiClient {
    /// Build a client from the resolved [`Config`].
    ///
    /// Never fails due to missing credentials — local services do not require
    /// authentication.  The only failure mode is a failure to build the
    /// `reqwest` connection pool (an OS-level resource exhaustion scenario).
    pub async fn new() -> anyhow::Result<Self> {
        let config = Config::load().await;

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            base_url:        config.api_url,
            gateway_url:     config.gateway_url,
            runtime_url:     config.runtime_url,
            data_engine_url: config.data_engine_url,
            queue_url:       config.queue_url,
        })
    }
}
