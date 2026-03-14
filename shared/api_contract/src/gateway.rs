use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

/// Basic route record (list/create responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RouteRow {
    pub id:           Uuid,
    pub path:         String,
    pub method:       String,
    pub function_id:  Uuid,
    pub is_async:     bool,
    pub auth_type:    String,
    pub cors_enabled: bool,
    pub rate_limit:   Option<i32>,
    pub created_at:   NaiveDateTime,
}

/// Extended route record with auth + CORS details (single-route GET response).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RouteFullRow {
    pub id:           Uuid,
    pub path:         String,
    pub method:       String,
    pub function_id:  Uuid,
    pub is_async:     bool,
    pub auth_type:    String,
    pub cors_enabled: bool,
    pub rate_limit:   Option<i32>,
    pub created_at:   NaiveDateTime,
    pub jwks_url:     Option<String>,
    pub jwt_audience: Option<String>,
    pub jwt_issuer:   Option<String>,
    pub cors_origins: Option<Vec<String>>,
    pub cors_headers: Option<Vec<String>>,
}

// ── Request payloads — routes ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateRoutePayload {
    pub method:       String,
    pub path:         String,
    pub function_id:  Uuid,
    #[serde(default)]
    pub is_async:     bool,
    pub auth_type:    String,
    pub cors_enabled: bool,
    pub rate_limit:   Option<i32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UpdateRoutePayload {
    pub path:         Option<String>,
    pub method:       Option<String>,
    pub function_id:  Option<Uuid>,
    pub is_async:     Option<bool>,
    pub auth_type:    Option<String>,
    pub cors_enabled: Option<bool>,
    pub rate_limit:   Option<Option<i32>>,
}

// ── Request payloads — gateway config ────────────────────────────────────────

/// Bulk route sync payload sent by `flux deploy` (gateway route table refresh).
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SyncRoutesPayload {
    pub project_deployment_id: Option<Uuid>,
    pub routes:                Vec<RoutePayloadEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RoutePayloadEntry {
    pub path:                  String,
    pub method:                String,
    pub function_name:         String,
    #[serde(default)]
    pub middleware:            Vec<String>,
    pub rate_limit_per_minute: Option<i32>,
}

/// Gateway routing table row — returned by `GET /routes` and `GET /internal/routes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RouteConfigRow {
    pub id:                    Uuid,
    pub path:                  String,
    pub method:                String,
    pub function_name:         String,
    pub middleware:            Vec<String>,
    pub rate_limit_per_minute: Option<i32>,
}

// ── Request payloads — middleware / rate-limit / CORS ────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RateLimitPayload {
    pub requests_per_second: i32,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CorsPayload {
    pub origins: Vec<String>,
    pub headers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MiddlewareCreatePayload {
    pub route_id:        Uuid,
    #[serde(rename = "type")]
    pub middleware_type: String,
    pub config:          Value,
}
