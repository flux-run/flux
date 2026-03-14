use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

/// A single API key record returned by list/create endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ApiKeyRow {
    pub id:           Uuid,
    pub name:         String,
    pub key_prefix:   String,
    pub created_at:   DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Returned by `POST /api-keys` — includes the full key (shown once only).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateApiKeyResponse {
    pub id:         Uuid,
    pub name:       String,
    pub key_prefix: String,
    pub key:        String,
    pub created_at: DateTime<Utc>,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateApiKeyPayload {
    pub name: String,
}
