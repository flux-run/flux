use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct EventRow {
    pub id:           Uuid,
    pub event_type:   String,
    pub table_name:   String,
    pub record_id:    Option<String>,
    pub operation:    String,
    pub payload:      Value,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at:   DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct EventSubscriptionRow {
    pub id:             Uuid,
    pub event_pattern:  String,
    pub target_type:    String,
    pub target_config:  Value,
    pub enabled:        bool,
    pub created_at:     DateTime<Utc>,
    pub updated_at:     DateTime<Utc>,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PublishEventPayload {
    pub event:   String,
    pub payload: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateSubscriptionPayload {
    pub event_pattern:  String,
    pub target_type:    String,
    pub target_config:  Option<Value>,
}
