//! Daily platform-log retention job.
//!
//! Hard-deletes old rows from `platform_logs` in batches of 1 000 to avoid
//! long table locks.
//!
//! Non-error rows use `record_retention_days`.
//! Error rows (level = 'error') use `error_retention_days` (defaults to 3×).
//!
//! The job sleeps until the next `job_hour_utc`, then runs, then sleeps again.
//! Safe to run in every replica simultaneously — each batch is independently
//! idempotent (delete where age > threshold).

use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use tokio::time::{interval_at, Duration, Instant};

use super::RetentionConfig;

const BATCH_SIZE: i64 = 1_000;

pub async fn run(pool: Arc<PgPool>, cfg: RetentionConfig) {
    if cfg.record_retention_days == 0 {
        tracing::info!("retention disabled (record_retention_days = 0)");
        return;
    }

    tracing::info!(
        record_days = cfg.record_retention_days,
        error_days = cfg.effective_error_days(),
        job_hour_utc = cfg.job_hour_utc,
        "retention worker started"
    );

    loop {
        let secs_until_next = seconds_until_hour(cfg.job_hour_utc);
        tracing::debug!(secs = secs_until_next, "retention: sleeping until next run");
        let start = Instant::now() + Duration::from_secs(secs_until_next);
        // interval_at fires immediately at `start`, then every 24 h
        let mut ticker = interval_at(start, Duration::from_secs(86_400));
        ticker.tick().await;

        if let Err(e) = run_once(&pool, cfg).await {
            tracing::warn!(error = %e, "retention job failed");
        }
    }
}

async fn run_once(pool: &PgPool, cfg: RetentionConfig) -> Result<(), sqlx::Error> {
    let success_cutoff = Utc::now()
        - chrono::Duration::days(cfg.record_retention_days as i64);
    let error_cutoff = Utc::now()
        - chrono::Duration::days(cfg.effective_error_days() as i64);

    let mut total: i64 = 0;

    // Delete non-error log rows in batches
    loop {
        let deleted = sqlx::query(
            "WITH batch AS (
                SELECT id FROM platform_logs
                WHERE level != 'error'
                  AND timestamp < $1
                LIMIT $2
            )
            DELETE FROM platform_logs
            WHERE id IN (SELECT id FROM batch)"
        )
        .bind(success_cutoff)
        .bind(BATCH_SIZE)
        .execute(pool)
        .await?
        .rows_affected() as i64;

        total += deleted;
        if deleted < BATCH_SIZE {
            break;
        }
        // Yield between batches to allow other queries through
        tokio::task::yield_now().await;
    }

    // Delete error log rows in batches (kept longer by default)
    loop {
        let deleted = sqlx::query(
            "WITH batch AS (
                SELECT id FROM platform_logs
                WHERE level = 'error'
                  AND timestamp < $1
                LIMIT $2
            )
            DELETE FROM platform_logs
            WHERE id IN (SELECT id FROM batch)"
        )
        .bind(error_cutoff)
        .bind(BATCH_SIZE)
        .execute(pool)
        .await?
        .rows_affected() as i64;

        total += deleted;
        if deleted < BATCH_SIZE {
            break;
        }
        tokio::task::yield_now().await;
    }

    if total > 0 {
        tracing::info!(
            deleted = total,
            success_cutoff = %success_cutoff.format("%Y-%m-%d"),
            error_cutoff   = %error_cutoff.format("%Y-%m-%d"),
            "Retention: deleted {} platform_log rows",
            total,
        );
    } else {
        tracing::debug!("retention: no records to delete");
    }

    Ok(())
}

/// Returns the number of seconds until the next occurrence of `hour` UTC.
fn seconds_until_hour(hour: u32) -> u64 {
    let now = Utc::now();
    let today_target = now
        .date_naive()
        .and_hms_opt(hour, 0, 0)
        .expect("valid hms");
    let target = chrono::TimeZone::from_utc_datetime(&Utc, &today_target);
    let diff = if target > now {
        (target - now).num_seconds()
    } else {
        // Already passed today — schedule for tomorrow
        (target + chrono::Duration::days(1) - now).num_seconds()
    };
    diff.max(0) as u64
}
