use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

/// A platform log entry returned by `GET /logs`.
///
/// `source`   — subsystem that emitted the log: `function | db | workflow | event | queue | system`
/// `resource` — resource identifier within that source (function name, table name, etc.)
/// `span_type`— span classification: `start | end | error | event`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PlatformLogRow {
    pub id:         Uuid,
    pub source:     String,
    pub resource:   String,
    pub level:      String,
    pub message:    String,
    pub request_id: Option<String>,
    pub span_type:  Option<String>,
    pub metadata:   Option<Value>,
    pub timestamp:  String,
    pub tier:       Option<String>,
}
