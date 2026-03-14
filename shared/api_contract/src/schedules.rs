use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CronJobRow {
    pub id:            Uuid,
    pub name:          String,
    pub schedule:      String,
    pub action_type:   String,
    pub action_config: Value,
    pub enabled:       bool,
    pub last_run_at:   Option<DateTime<Utc>>,
    pub next_run_at:   Option<DateTime<Utc>>,
    pub created_at:    DateTime<Utc>,
    pub updated_at:    DateTime<Utc>,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateSchedulePayload {
    pub name:          String,
    pub schedule:      String,
    pub action_type:   String,
    pub action_config: Option<Value>,
}
