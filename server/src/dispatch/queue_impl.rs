//! In-process implementation of [`QueueDispatch`].
//!
//! Calls the queue crate's `job_service::create_job` directly — no HTTP.

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use job_contract::dispatch::QueueDispatch;
use flux_queue::services::job_service::{self, CreateJobInput};

/// Pushes jobs directly into the `flux.jobs` table — used by the monolithic
/// server binary so V8 `ctx.queue.push()` never makes an HTTP call.
pub struct InProcessQueueDispatch {
    pub pool: PgPool,
}

#[async_trait]
impl QueueDispatch for InProcessQueueDispatch {
    async fn push_job(
        &self,
        function_id:     &str,
        payload:         Value,
        delay_seconds:   Option<u64>,
        idempotency_key: Option<String>,
    ) -> Result<(), String> {
        let fid: Uuid = function_id
            .parse()
            .map_err(|e| format!("invalid function_id '{}': {}", function_id, e))?;

        let run_at = match delay_seconds {
            Some(d) if d > 0 => {
                chrono::Utc::now().naive_utc()
                    + chrono::Duration::try_seconds(d as i64).unwrap_or_default()
            }
            _ => chrono::Utc::now().naive_utc(),
        };

        let input = CreateJobInput {
            function_id: fid,
            payload,
            run_at,
            max_attempts: 5,
            idempotency_key,
        };

        job_service::create_job(&self.pool, input)
            .await
            .map_err(|e| format!("push_job failed: {}", e))?;

        Ok(())
    }
}
