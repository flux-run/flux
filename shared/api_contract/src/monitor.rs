//! Monitor / alerting contract types.

use serde::{Deserialize, Serialize};

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateAlertPayload {
    pub name:      String,
    /// One of: `"error_rate"` | `"latency_p95"` | `"latency_p99"` |
    ///         `"queue_dlq"` | `"queue_failed"` | `"queue_pending"`
    pub metric:    String,
    pub threshold: f64,
    /// `"above"` | `"below"` (default: `"above"`)
    #[serde(default = "default_condition")]
    pub condition: String,
    /// Evaluation window in seconds, 60–86400 (default: 300)
    #[serde(default = "default_window")]
    pub window_secs: i32,
}

fn default_condition() -> String { "above".into() }
fn default_window()    -> i32    { 300 }
