use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Shared summary types ──────────────────────────────────────────────────────

/// Per-function result inside a project deployment summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct FunctionDeploySummaryEntry {
    pub name:    String,
    pub version: i64,
    /// `"deployed"` | `"skipped"` | `"failed"`
    pub status:  String,
}

/// Aggregate counts included in project deployment records.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DeploySummary {
    pub total:     i64,
    pub deployed:  i64,
    pub skipped:   i64,
    pub functions: Vec<FunctionDeploySummaryEntry>,
}

// ── Response types ────────────────────────────────────────────────────────────

/// A single deployment record returned by the list/get endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DeploymentResponse {
    pub id:         Uuid,
    pub version:    i32,
    pub is_active:  bool,
    pub status:     String,
    pub created_at: String,
    pub run_url:    Option<String>,
}

// ── Request payloads ──────────────────────────────────────────────────────────

/// `POST /functions/{id}/deployments`
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateDeploymentPayload {
    pub storage_key: String,
}

/// `POST /deployments/project` — records a multi-function deploy in one call.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateProjectDeploymentPayload {
    pub version:     i64,
    pub summary:     DeploySummary,
    pub deployed_by: Option<String>,
}
