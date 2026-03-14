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

impl Job {
    /// Returns `true` when the job has exhausted its retry budget.
    pub fn is_exhausted(&self) -> bool {
        self.attempts >= self.max_attempts
    }

    /// Returns `true` when the job is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        self.status == "completed" || self.status == "dead_letter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_job(status: &str, attempts: i32, max_attempts: i32) -> Job {
        let now = Utc::now().naive_utc();
        Job {
            id:                  Uuid::new_v4(),
            function_id:         Uuid::new_v4(),
            payload:             serde_json::json!({}),
            status:              status.into(),
            attempts,
            max_attempts,
            max_runtime_seconds: 30,
            run_at:              now,
            locked_at:           None,
            started_at:          None,
            request_id:          None,
            idempotency_key:     None,
            created_at:          now,
            updated_at:          now,
        }
    }

    #[test]
    fn job_not_exhausted_below_max() {
        let j = make_job("running", 2, 5);
        assert!(!j.is_exhausted());
    }

    #[test]
    fn job_exhausted_at_max() {
        let j = make_job("running", 5, 5);
        assert!(j.is_exhausted());
    }

    #[test]
    fn job_exhausted_above_max() {
        let j = make_job("running", 6, 5);
        assert!(j.is_exhausted());
    }

    #[test]
    fn completed_job_is_terminal() {
        assert!(make_job("completed", 1, 5).is_terminal());
    }

    #[test]
    fn dead_letter_job_is_terminal() {
        assert!(make_job("dead_letter", 5, 5).is_terminal());
    }

    #[test]
    fn pending_job_is_not_terminal() {
        assert!(!make_job("pending", 0, 5).is_terminal());
    }

    #[test]
    fn running_job_is_not_terminal() {
        assert!(!make_job("running", 1, 5).is_terminal());
    }

    #[test]
    fn job_serialises_to_json() {
        let j = make_job("pending", 0, 3);
        let s = serde_json::to_string(&j).unwrap();
        assert!(s.contains("\"status\":\"pending\""));
        assert!(s.contains("\"attempts\":0"));
    }
}