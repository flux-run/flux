use serde::{Deserialize, Serialize};

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrateRequest {
    /// Filename, e.g. `001_create_users.sql`. Used as the unique migration key.
    pub name:    String,
    /// Full SQL content of the migration file.
    pub content: String,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrateResponse {
    /// `"applied"` or `"already_applied"`
    pub status:  String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
