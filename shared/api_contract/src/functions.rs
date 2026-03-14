use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Response types ────────────────────────────────────────────────────────────

/// Full function record returned by list/get endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct FunctionResponse {
    pub id:            Uuid,
    pub name:          String,
    pub runtime:       String,
    pub description:   Option<String>,
    pub input_schema:  Option<Value>,
    pub output_schema: Option<Value>,
    pub created_at:    String,
    pub run_url:       String,
}

/// Returned by `POST /functions`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateFunctionResponse {
    pub function_id: Uuid,
    pub name:        String,
    pub runtime:     String,
    pub run_url:     String,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateFunctionPayload {
    pub name:    String,
    pub runtime: Option<String>,
}
