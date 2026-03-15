//! HTTP transport for the Flux CLI.
//!
//! `ApiClient` is a thin wrapper around `reqwest::Client` pre-wired to the
//! Flux server URL.  When a `cli_key` is configured (from `.flux/config.json`
//! or `FLUX_CLI_KEY`), it is sent as `Authorization: Bearer <key>` on every
//! request so the server can verify the request originates from the CLI.
//!
//! # URL resolution (highest precedence first)
//! 1. `FLUX_URL` env var
//! 2. `.flux/config.json` `server_url` in project tree
//! 3. `FLUX_API_URL` env var
//! 4. `~/.flux/config.json`
//! 5. Hard-coded default: http://localhost:4000/flux/api

use std::time::Duration;

use reqwest::{Client, header};

use crate::config::Config;

// ─── ApiClient ────────────────────────────────────────────────────────────────

/// HTTP client pre-wired to the local Flux server.
///
/// Responsibilities (Single Responsibility Principle):
/// - Hold a single `reqwest::Client` (connection-pool reuse)
/// - Expose the resolved service base URLs
/// - Inject `Authorization: Bearer <cli_key>` when a key is configured
///
/// This type intentionally does NOT:
/// - Know about tenants or projects
/// - Assume a remote endpoint
pub struct ApiClient {
    /// Underlying reqwest connection pool (default headers include auth).
    pub client: Client,
    /// Flux server API base — `http://localhost:4000/flux/api`
    pub base_url: String,
    /// Gateway / function invocation base — `http://localhost:4000`
    pub gateway_url: String,
}

impl ApiClient {
    /// Build a client from the resolved [`Config`].
    ///
    /// If `cli_key` is set the reqwest client will send
    /// `Authorization: Bearer <key>` as a default header on every request,
    /// so individual command modules don't need to handle auth themselves.
    pub async fn new() -> anyhow::Result<Self> {
        let config = Config::load().await;

        // Build default headers — inject auth key when configured.
        let mut default_headers = header::HeaderMap::new();
        if let Some(ref key) = config.cli_key {
            if !key.is_empty() {
                let value = format!("Bearer {}", key);
                if let Ok(hv) = header::HeaderValue::from_str(&value) {
                    default_headers.insert(header::AUTHORIZATION, hv);
                }
            }
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .default_headers(default_headers)
            .build()?;

        Ok(Self {
            client,
            base_url:    config.api_url,
            gateway_url: config.gateway_url,
        })
    }
}
