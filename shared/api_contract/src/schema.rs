//! Schema push contract types — used by `POST /internal/db/schema`
//! which is called by `flux db push` to register table metadata with the
//! data engine.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SchemaManifest {
    pub table:        String,
    pub file:         Option<String>,
    pub columns:      Value,
    pub indexes:      Option<Value>,
    pub foreign_keys: Option<Value>,
    pub rules:        Option<Value>,
    pub hooks:        Option<Value>,
    pub on:           Option<Value>,
}
