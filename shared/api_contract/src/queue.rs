use chrono::NaiveDateTime;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct QueueConfigRow {
    pub id:                   Uuid,
    pub name:                 String,
    pub description:          Option<String>,
    pub max_attempts:         i32,
    pub visibility_timeout_ms: i64,
    pub created_at:           DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DeadLetterJobRow {
    pub id:          Uuid,
    pub function_id: Option<Uuid>,
    pub payload:     Option<Value>,
    pub error:       Option<String>,
    pub failed_at:   Option<NaiveDateTime>,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateQueuePayload {
    pub name:                 String,
    pub description:          Option<String>,
    pub max_attempts:         Option<i32>,
    pub visibility_timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PublishMessagePayload {
    pub function_id:    Uuid,
    pub payload:        Option<Value>,
    pub delay_seconds:  Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct QueueBindingRow {
    pub id:          Uuid,
    pub queue_name:  String,
    pub function_id: Uuid,
    pub created_at:  DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateBindingPayload {
    pub function_id: Uuid,
}
