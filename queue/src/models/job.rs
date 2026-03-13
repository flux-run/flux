//! Job model — the central record type for the queue system.
//!
//! ## Job lifecycle states
//!
//! ```text
//! pending ──► running ──► completed
//!    ▲            │
//!    │            ├──► pending  (retry, attempts < max_attempts)
//!    │            │
//!    └────────────┴──► dead_letter_jobs  (attempts >= max_attempts, or timeout)
//! ```
//!
//! State transitions are performed by:
//! - `fetch_and_lock_jobs` — `pending` → `running` (atomic SELECT … FOR UPDATE SKIP LOCKED)
//! - `update_status`       — `running` → `completed`
//! - `retry_service`       — `running` → `pending`  (re-schedules with backoff)
//! - `retry_service`       — `running` → `dead_letter_jobs` (moves row to separate table)
//! - `timeout_recovery`    — `running` → `pending` or `dead_letter_jobs` (stuck job rescue)
//!
//! ## request_id
//!
//! `request_id` is stamped at the start of execution (not at enqueue time) so all spans
//! emitted by the runtime, data-engine, and hooks during a single job run share one ID.
//! This enables `flux trace <request_id>` to reconstruct the full execution timeline.
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(FromRow, Serialize, Deserialize, Clone)]
pub struct Job {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub function_id: Uuid,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub max_runtime_seconds: i32,
    pub run_at: NaiveDateTime,
    pub locked_at: Option<NaiveDateTime>,
    pub started_at: Option<NaiveDateTime>,
    /// UUID sent as `x-request-id` to the runtime. Links this job to its
    /// execution record so `flux trace <request_id>` shows the full trace.
    pub request_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}