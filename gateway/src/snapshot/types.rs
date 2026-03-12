//! Core data types for the in-memory route snapshot.
use std::collections::HashMap;
use uuid::Uuid;
use serde::Serialize;

/// A single registered route as loaded from the database.
///
/// Fields map directly to the `routes` table joined with `functions`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RouteRecord {
    pub id:           Uuid,
    pub project_id:   Uuid,
    pub function_id:  Uuid,
    pub path:         String,
    pub method:       String,
    /// Runtime engine: "deno" (default) or "wasm".
    pub runtime:      String,
    pub auth_type:    String,
    pub cors_enabled: bool,
    /// Per-route rate limit (req/s).  Falls back to the global default when None.
    pub rate_limit:   Option<i32>,
    pub jwks_url:     Option<String>,
    pub jwt_audience: Option<String>,
    pub jwt_issuer:   Option<String>,
    /// Optional JSON Schema for request body validation.
    pub json_schema:  Option<serde_json::Value>,
    pub cors_origins: Option<Vec<String>>,
    pub cors_headers: Option<Vec<String>>,
}

/// The entire routing table held in memory.
///
/// Key: `(HTTP_METHOD_UPPERCASE, /path)` — simple, no tenant scoping.
#[derive(Default, Clone)]
pub struct SnapshotData {
    pub routes: HashMap<(String, String), RouteRecord>,
}
