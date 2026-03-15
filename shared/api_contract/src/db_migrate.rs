use serde::{Deserialize, Serialize};

fn default_database() -> String { "default".into() }

// ── Single-file apply (internal: flux db-push) ────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrateRequest {
    /// Filename, e.g. `001_create_users.sql`. Used as the unique migration key.
    pub name:    String,
    /// Full SQL content of the migration file.
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrateResponse {
    /// `"applied"` or `"already_applied"`
    pub status:  String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Batch migration operations (flux db migration apply/rollback/status) ──────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationApplyRequest {
    #[serde(default = "default_database")]
    pub database: String,
    /// Apply at most this many pending migrations. `None` means apply all.
    pub count: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationApplyResponse {
    pub applied: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationRollbackRequest {
    #[serde(default = "default_database")]
    pub database: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationRollbackResponse {
    pub rolled_back: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationStatusRow {
    pub name:    String,
    pub applied: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MigrationStatusResponse {
    pub migrations: Vec<MigrationStatusRow>,
}
