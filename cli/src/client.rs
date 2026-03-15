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

use api_contract::routes::Route;
use reqwest::{Client, StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};

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

    // ── Internal ─────────────────────────────────────────────────────────────

    /// Drive a pre-built `RequestBuilder` to completion and deserialise the
    /// JSON response body as `Resp`.  Handles `204 No Content` by returning
    /// `serde_json::Value::Null` (or `()`) rather than failing to parse an
    /// empty body.
    async fn run<Resp: DeserializeOwned>(req: reqwest::RequestBuilder) -> anyhow::Result<Resp> {
        let res = req.send().await?.error_for_status()?;
        if res.status() == StatusCode::NO_CONTENT {
            Ok(serde_json::from_value(serde_json::Value::Null)?)
        } else {
            Ok(res.json::<Resp>().await?)
        }
    }

    // ── GET ──────────────────────────────────────────────────────────────────

    /// `GET /path` — typed response enforced by `Route<(), Resp>`.
    pub async fn get<Resp: DeserializeOwned>(&self, route: &Route<(), Resp>) -> anyhow::Result<Resp> {
        Self::run(self.client.get(route.url(&self.base_url))).await
    }

    /// `GET /path/{param}?key=val` — path substitution via `path_params`;
    /// optional query parameters via `query` (any `serde::Serialize` value,
    /// e.g. `&[("limit", "100")]` or a `Vec<(String, String)>`).
    pub async fn get_with<Resp, Q>(&self, route: &Route<(), Resp>, path_params: &[(&str, &str)], query: &Q) -> anyhow::Result<Resp>
    where
        Resp: DeserializeOwned,
        Q: Serialize + ?Sized,
    {
        let url = route.url_with(&self.base_url, path_params);
        Self::run(self.client.get(url).query(query)).await
    }

    // ── POST ─────────────────────────────────────────────────────────────────

    /// `POST /path` — `Req` body type enforced by `Route<Req, Resp>`.
    pub async fn post<Req, Resp>(&self, route: &Route<Req, Resp>, body: &Req) -> anyhow::Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        Self::run(self.client.post(route.url(&self.base_url)).json(body)).await
    }

    /// `POST /path/{param}` — path substitution + typed body.
    pub async fn post_with<Req, Resp>(&self, route: &Route<Req, Resp>, path_params: &[(&str, &str)], body: &Req) -> anyhow::Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        let url = route.url_with(&self.base_url, path_params);
        Self::run(self.client.post(url).json(body)).await
    }

    // ── PUT ──────────────────────────────────────────────────────────────────

    /// `PUT /path/{param}` — typed body.
    pub async fn put<Req, Resp>(&self, route: &Route<Req, Resp>, path_params: &[(&str, &str)], body: &Req) -> anyhow::Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        let url = route.url_with(&self.base_url, path_params);
        Self::run(self.client.put(url).json(body)).await
    }

    // ── PATCH ────────────────────────────────────────────────────────────────

    /// `PATCH /path/{param}` — typed body.
    pub async fn patch<Req, Resp>(&self, route: &Route<Req, Resp>, path_params: &[(&str, &str)], body: &Req) -> anyhow::Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        let url = route.url_with(&self.base_url, path_params);
        Self::run(self.client.patch(url).json(body)).await
    }

    // ── DELETE ───────────────────────────────────────────────────────────────

    /// `DELETE /path`.
    pub async fn delete<Resp: DeserializeOwned>(&self, route: &Route<(), Resp>) -> anyhow::Result<Resp> {
        Self::run(self.client.delete(route.url(&self.base_url))).await
    }

    /// `DELETE /path/{param}`.
    pub async fn delete_with<Resp: DeserializeOwned>(&self, route: &Route<(), Resp>, path_params: &[(&str, &str)]) -> anyhow::Result<Resp> {
        Self::run(self.client.delete(route.url_with(&self.base_url, path_params))).await
    }

    /// `DELETE /path?key=val` — with query params but no path substitution.
    pub async fn delete_q<Resp, Q>(&self, route: &Route<(), Resp>, query: &Q) -> anyhow::Result<Resp>
    where
        Resp: DeserializeOwned,
        Q: Serialize + ?Sized,
    {
        Self::run(self.client.delete(route.url(&self.base_url)).query(query)).await
    }
}
