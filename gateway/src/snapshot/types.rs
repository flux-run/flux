//! Core data types for the in-memory route snapshot.
//!
//! [`RouteRecord`] is the primary type: one row per registered route, loaded
//! from `routes JOIN functions` and held in a [`SnapshotData`] hash map.
//! The key is `(HTTP_METHOD_UPPERCASE, /path)` — no tenant scoping because
//! routes are globally unique within a project by (method, path).
use std::collections::HashMap;
use uuid::Uuid;
use serde::Serialize;

/// A single registered route as loaded from the database.
///
/// Fields map directly to the `routes` table joined with `functions`.
/// Column sources are noted inline.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RouteRecord {
    /// Primary key from `routes.id` — used as part of the rate-limit key.
    pub id:           Uuid,
    /// `routes.project_id` — used to scope the trace root DB write.
    pub project_id:   Uuid,
    /// `routes.function_id` — sent to the runtime to identify which function to run.
    pub function_id:  Uuid,
    /// `routes.path` — the URL path this route matches (e.g. `/hello`).
    pub path:         String,
    /// `routes.method` — stored uppercase (GET, POST, …); matched case-insensitively.
    pub method:       String,
    /// `functions.runtime` — runtime engine: `"deno"` (default) or `"wasm"`.
    /// Forwarded to the runtime as the `X-Function-Runtime` header.
    pub runtime:      String,
    /// `routes.auth_type` — one of `"none"`, `"api_key"`, `"jwt"`.
    pub auth_type:    String,
    /// `routes.cors_enabled` — when true, CORS headers are injected and
    /// OPTIONS preflight is answered without authentication.
    pub cors_enabled: bool,
    /// `routes.rate_limit` — per-route override (req/s).
    /// `None` means use the global `GatewayState::rate_limit_per_sec` default.
    pub rate_limit:   Option<i32>,
    /// `routes.jwks_url` — required when `auth_type = "jwt"`.
    /// Points to the JWKS endpoint used to verify the incoming JWT.
    pub jwks_url:     Option<String>,
    /// `routes.jwt_audience` — optional `aud` claim check during JWT validation.
    pub jwt_audience: Option<String>,
    /// `routes.jwt_issuer` — optional `iss` claim check during JWT validation.
    pub jwt_issuer:   Option<String>,
    /// `routes.json_schema` — optional JSON Schema (draft-07+) that the request
    /// body must satisfy.  Validation runs after the body is fully buffered.
    pub json_schema:  Option<serde_json::Value>,
    /// `routes.cors_origins` — allowed `Origin` values for CORS responses.
    /// `None` or empty falls back to `*`.
    pub cors_origins: Option<Vec<String>>,
    /// `routes.cors_headers` — allowed request headers for CORS preflight.
    /// `None` or empty falls back to `Content-Type, Authorization, X-API-Key`.
    pub cors_headers: Option<Vec<String>>,
}

/// The entire routing table held in memory.
///
/// Key: `(HTTP_METHOD_UPPERCASE, /path)` — simple, no tenant scoping.
#[derive(Default, Clone)]
pub struct SnapshotData {
    pub routes: HashMap<(String, String), RouteRecord>,
}
