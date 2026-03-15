use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use uuid::Uuid;

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
         FROM flux_internal.cron_jobs \
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

        let request_id = format!("cron:{}", job_id);
        if let Err(e) = dispatch_function(http, runtime_url, &action_type, &action_config, &payload, &request_id).await {
            tracing::warn!(job_id = %job_id, name = %name, error = %e, "cron dispatch failed");
        }

        // Compute next run time regardless of dispatch success.
        let next = compute_next_run(&schedule);

        sqlx::query(
            "UPDATE flux_internal.cron_jobs \
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

/// Dispatch a cron job to the runtime by calling `POST /internal/execute`.
async fn dispatch_function(
    http: &reqwest::Client,
    runtime_url: &str,
    action_type: &str,
    config: &serde_json::Value,
    payload: &serde_json::Value,
    request_id: &str,
) -> Result<(), String> {
    if action_type != "function" {
        return Err(format!("unsupported cron action_type: {}", action_type));
    }
    let function_id = config
        .get("function_id")
        .and_then(|v| v.as_str())
        .ok_or("cron action_config missing 'function_id'")?;

    let body = serde_json::json!({
        "function_id": function_id,
        "payload": payload,
    });

    let resp = http
        .post(format!("{}/internal/execute", runtime_url))
        .header("x-request-id", request_id)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("cron dispatch failed: {}", e))?;

    let status = resp.status().as_u16();
    if status < 400 {
        Ok(())
    } else {
        Err(format!("runtime returned HTTP {}", status))
    }
}

/// Compute the next run time from a 5-field cron expression.
fn compute_next_run(schedule: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use std::str::FromStr;
    let schedule = format!("0 {}", schedule); // cron crate uses 6-field (with seconds first)
    let cron_schedule = cron::Schedule::from_str(&schedule).ok()?;
    cron_schedule.upcoming(chrono::Utc).next()
}
