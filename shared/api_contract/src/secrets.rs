use serde::{Deserialize, Serialize};

// ── Response types ────────────────────────────────────────────────────────────

/// Secret record — value is never included in responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SecretResponse {
    pub key:        String,
    pub version:    i32,
    pub created_at: String,
}

// ── Request payloads ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateSecretRequest {
    pub key:   String,
    pub value: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UpdateSecretRequest {
    pub value: String,
}
