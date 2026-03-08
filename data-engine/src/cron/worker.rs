use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use uuid::Uuid;

use crate::events::dispatcher;

/// Background task: dispatch cron jobs whose `next_run_at` has passed.
///
/// Runs every 30 seconds. PostgreSQL row locking (`FOR UPDATE SKIP LOCKED`)
/// makes it safe to run this in every data-engine replica simultaneously.
pub async fn run(pool: Arc<PgPool>, http: Arc<reqwest::Client>, runtime_url: String) {
    let mut tick = interval(Duration::from_secs(30));
    tracing::info!("cron worker started");

    loop {
        tick.tick().await;
        if let Err(e) = fire_due_jobs(&pool, &http, &runtime_url).await {
            tracing::warn!(error = %e, "cron worker error");
        }
    }
}

async fn fire_due_jobs(pool: &PgPool, http: &reqwest::Client, runtime_url: &str) -> Result<(), sqlx::Error> {
    let jobs = sqlx::query(
        "SELECT id, name, action_type, action_config, schedule \
         FROM fluxbase_internal.cron_jobs \
         WHERE enabled = TRUE \
           AND next_run_at IS NOT NULL \
           AND next_run_at <= now() \
         LIMIT 50 \
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(pool)
    .await?;

    if jobs.is_empty() {
        return Ok(());
    }

    tracing::debug!(count = jobs.len(), "firing cron jobs");

    for job in &jobs {
        let job_id: Uuid = job.get("id");
        let name: String = job.get("name");
        let action_type: String = job.get("action_type");
        let action_config: serde_json::Value = job.get("action_config");
        let schedule: String = job.get("schedule");

        let triggered_at = chrono::Utc::now();
        let payload = serde_json::json!({
            "cron_job_id": job_id,
            "cron_job_name": name,
            "triggered_at": triggered_at.to_rfc3339(),
        });

        let result = dispatcher::dispatch(
            pool, http, runtime_url,
            job_id, // reused as identifier
            &action_type,
            &action_config,
            &payload,
            "cron.fired",
        )
        .await;

        if let Err(ref e) = result {
            tracing::warn!(job_id = %job_id, name = %name, error = %e, "cron dispatch failed");
        }

        // Compute next run time regardless of dispatch success.
        let next = compute_next_run(&schedule);

        sqlx::query(
            "UPDATE fluxbase_internal.cron_jobs \
             SET last_run_at = now(), next_run_at = $1 \
             WHERE id = $2",
        )
        .bind(next)
        .bind(job_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Compute the next run time from a 5-field cron expression.
/// Returns None if the expression is invalid (job won't fire again until fixed).
fn compute_next_run(schedule: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use std::str::FromStr;
    let schedule = format!("0 {}", schedule); // cron crate uses 6-field (with seconds first)
    let cron_schedule = cron::Schedule::from_str(&schedule).ok()?;
    cron_schedule.upcoming(chrono::Utc).next()
}
