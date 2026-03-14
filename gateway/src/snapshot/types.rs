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
/// Fields map directly to the `flux.routes` table.
/// Column sources are noted inline.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RouteRecord {
    /// Primary key from `flux.routes.id` — used as part of the rate-limit key.
    pub id:           Uuid,
    /// `flux.routes.function_name` — name of the function to invoke.
    pub function_name: String,
    /// `flux.routes.path` — the URL path this route matches (e.g. `/hello`).
    pub path:         String,
    /// `flux.routes.method` — stored uppercase (GET, POST, …).
    pub method:       String,
    /// `flux.routes.auth_type` — one of `"none"`, `"api_key"`, `"jwt"`.
    pub auth_type:    String,
    /// `flux.routes.cors_enabled` — when true, CORS headers are injected.
    pub cors_enabled: bool,
    /// `flux.routes.rate_limit_per_minute` — per-route override.
    pub rate_limit_per_minute: Option<i32>,
    /// `flux.routes.jwks_url` — required when `auth_type = "jwt"`.
    pub jwks_url:     Option<String>,
    /// `flux.routes.jwt_audience` — optional `aud` claim check.
    pub jwt_audience: Option<String>,
    /// `flux.routes.jwt_issuer` — optional `iss` claim check.
    pub jwt_issuer:   Option<String>,
    /// `flux.routes.json_schema` — optional JSON Schema for request body validation.
    pub json_schema:  Option<serde_json::Value>,
    /// `flux.routes.cors_origins` — allowed `Origin` values for CORS responses.
    pub cors_origins: Option<Vec<String>>,
    /// `flux.routes.cors_headers` — allowed request headers for CORS preflight.
    pub cors_headers: Option<Vec<String>>,
}

/// The entire routing table held in memory.
///
/// Key: `(HTTP_METHOD_UPPERCASE, /path)` — simple, no tenant scoping.
#[derive(Default, Clone)]
pub struct SnapshotData {
    pub routes: HashMap<(String, String), RouteRecord>,
}
