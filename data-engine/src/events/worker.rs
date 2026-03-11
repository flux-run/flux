use reqwest::Client;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use uuid::Uuid;

use super::dispatcher;

const BATCH_SIZE: i64 = 50;
const MAX_ATTEMPTS: i32 = 5;

/// Background task: two interleaved loops sharing the same tick.
///
/// 1. New events loop  — claim undelivered events, fan out to subscriptions.
/// 2. Retry loop       — re-dispatch delivery rows whose `next_attempt_at` has passed.
pub async fn run(pool: Arc<PgPool>, http: Arc<Client>, runtime_url: String) {
    let mut tick = interval(Duration::from_secs(1));
    tracing::info!("event worker started");

    loop {
        tick.tick().await;

        if let Err(e) = process_new_events(&pool, &http, &runtime_url).await {
            tracing::warn!(error = %e, "new-event batch error");
        }

        if let Err(e) = process_retries(&pool, &http, &runtime_url).await {
            tracing::warn!(error = %e, "retry batch error");
        }
    }
}

// ─── New-event loop ───────────────────────────────────────────────────────────

async fn process_new_events(pool: &PgPool, http: &Client, runtime_url: &str) -> Result<(), sqlx::Error> {
    // Claim a locked batch of undelivered events.
    let events = sqlx::query(
        "SELECT id, tenant_id, project_id, event_type, payload, request_id \
         FROM fluxbase_internal.events \
         WHERE delivered_at IS NULL \
         ORDER BY created_at \
         LIMIT $1 \
         FOR UPDATE SKIP LOCKED",
    )
    .bind(BATCH_SIZE)
    .fetch_all(pool)
    .await?;

    if events.is_empty() {
        return Ok(());
    }

    tracing::debug!(count = events.len(), "processing new events");

    for event in &events {
        let event_id: Uuid = event.get("id");
        let tenant_id: Uuid = event.get("tenant_id");
        let project_id: Uuid = event.get("project_id");
        let event_type: String = event.get("event_type");
        let payload: serde_json::Value = event.get("payload");
        let request_id: Option<String> = event.try_get("request_id").unwrap_or(None);
        let request_id_str: String = request_id.unwrap_or_else(|| event_id.to_string());

        let subs = load_matching_subscriptions(pool, tenant_id, project_id, &event_type).await?;

        if subs.is_empty() {
            // No subscriptions — mark delivered immediately.
            mark_delivered(pool, event_id).await;
            continue;
        }

        // Pre-reserve a delivery row for every matching subscription.
        // This way a crash between reservation and dispatch leaves rows in
        // 'pending', which the retry loop will pick up.
        let delivery_ids = reserve_deliveries(pool, event_id, &subs).await?;

        let mut all_terminal = true;

        for (delivery_id, sub) in delivery_ids.iter().zip(subs.iter()) {
            let sub_id: Uuid = sub.get("id");
            let target_type: String = sub.get("target_type");
            let target_config: serde_json::Value = sub.get("target_config");
            let max_attempts: i32 = sub.get("max_attempts");

            let result = dispatcher::dispatch(
                pool, http, runtime_url, sub_id,
                &target_type, &target_config, &payload, &event_type,
                &request_id_str,
            )
            .await;

            match result {
                Ok(status) => {
                    update_delivery(pool, *delivery_id, "success", status, None, None).await;
                }
                Err(ref e) => {
                    if max_attempts <= 1 {
                        update_delivery(pool, *delivery_id, "dead_letter", 0, Some(e), None).await;
                    } else {
                        // Schedule first retry: 2 seconds from now.
                        let next = Some(retry_delay_secs(1));
                        update_delivery(pool, *delivery_id, "failed", 0, Some(e), next).await;
                        all_terminal = false;
                    }
                    tracing::warn!(
                        event_id = %event_id, subscription_id = %sub_id,
                        error = %e, "dispatch failed"
                    );
                }
            }
        }

        // Only mark the event delivered when every delivery is in a terminal
        // state. If any are scheduled for retry, we'll check again later.
        if all_terminal {
            mark_delivered(pool, event_id).await;
        }
    }

    Ok(())
}

// ─── Retry loop ───────────────────────────────────────────────────────────────

async fn process_retries(pool: &PgPool, http: &Client, runtime_url: &str) -> Result<(), sqlx::Error> {
    // Claim delivery rows that are due for retry.
    let retries = sqlx::query(
        "SELECT d.id, d.event_id, d.subscription_id, d.attempt, \
                s.target_type, s.target_config, s.max_attempts, \
                e.event_type, e.payload, e.request_id \
         FROM fluxbase_internal.event_deliveries d \
         JOIN fluxbase_internal.event_subscriptions s ON s.id = d.subscription_id \
         JOIN fluxbase_internal.events e ON e.id = d.event_id \
         WHERE d.status = 'failed' \
           AND d.next_attempt_at IS NOT NULL \
           AND d.next_attempt_at <= now() \
         ORDER BY d.next_attempt_at \
         LIMIT $1 \
         FOR UPDATE OF d SKIP LOCKED",
    )
    .bind(BATCH_SIZE)
    .fetch_all(pool)
    .await?;

    if retries.is_empty() {
        return Ok(());
    }

    tracing::debug!(count = retries.len(), "processing delivery retries");

    for row in &retries {
        let delivery_id: Uuid = row.get("id");
        let event_id: Uuid = row.get("event_id");
        let sub_id: Uuid = row.get("subscription_id");
        let attempt: i32 = row.get("attempt");
        let max_attempts: i32 = row.get("max_attempts");
        let target_type: String = row.get("target_type");
        let target_config: serde_json::Value = row.get("target_config");
        let event_type: String = row.get("event_type");
        let payload: serde_json::Value = row.get("payload");
        let request_id: Option<String> = row.try_get("request_id").unwrap_or(None);
        let request_id_str: String = request_id.unwrap_or_else(|| event_id.to_string());

        let next_attempt = attempt + 1;

        let result = dispatcher::dispatch(
            pool, http, runtime_url, sub_id,
            &target_type, &target_config, &payload, &event_type,
            &request_id_str,
        )
        .await;

        match result {
            Ok(status) => {
                // Bump attempt count, mark success.
                sqlx::query(
                    "UPDATE fluxbase_internal.event_deliveries \
                     SET status = 'success', response_status = $1, attempt = $2, \
                         dispatched_at = now(), next_attempt_at = NULL \
                     WHERE id = $3",
                )
                .bind(status as i32)
                .bind(next_attempt)
                .bind(delivery_id)
                .execute(pool)
                .await?;

                check_and_mark_event_delivered(pool, event_id).await;
            }
            Err(ref e) => {
                if next_attempt >= max_attempts {
                    sqlx::query(
                        "UPDATE fluxbase_internal.event_deliveries \
                         SET status = 'dead_letter', error_message = $1, attempt = $2, \
                             next_attempt_at = NULL \
                         WHERE id = $3",
                    )
                    .bind(e.as_str())
                    .bind(next_attempt)
                    .bind(delivery_id)
                    .execute(pool)
                    .await?;

                    tracing::warn!(
                        delivery_id = %delivery_id, subscription_id = %sub_id,
                        attempt = next_attempt, "delivery dead-lettered"
                    );

                    check_and_mark_event_delivered(pool, event_id).await;
                } else {
                    let next_at = retry_delay_secs(next_attempt);
                    sqlx::query(
                        "UPDATE fluxbase_internal.event_deliveries \
                         SET status = 'failed', error_message = $1, attempt = $2, \
                             next_attempt_at = now() + $3 * INTERVAL '1 second' \
                         WHERE id = $4",
                    )
                    .bind(e.as_str())
                    .bind(next_attempt)
                    .bind(next_at)
                    .bind(delivery_id)
                    .execute(pool)
                    .await?;
                }
            }
        }
    }

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Pre-insert 'pending' delivery rows for every subscription before dispatching.
/// Returns the new delivery UUIDs in subscription order.
async fn reserve_deliveries(
    pool: &PgPool,
    event_id: Uuid,
    subs: &[sqlx::postgres::PgRow],
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut ids = Vec::with_capacity(subs.len());
    for sub in subs {
        let sub_id: Uuid = sub.get("id");
        let row = sqlx::query(
            "INSERT INTO fluxbase_internal.event_deliveries \
                 (event_id, subscription_id, status) \
             VALUES ($1, $2, 'pending') \
             RETURNING id",
        )
        .bind(event_id)
        .bind(sub_id)
        .fetch_one(pool)
        .await?;
        ids.push(row.get("id"));
    }
    Ok(ids)
}

async fn update_delivery(
    pool: &PgPool,
    delivery_id: Uuid,
    status: &str,
    http_status: u16,
    error: Option<&str>,
    next_attempt_at_secs: Option<i64>,
) {
    let _ = if let Some(secs) = next_attempt_at_secs {
        sqlx::query(
            "UPDATE fluxbase_internal.event_deliveries \
             SET status = $1, response_status = $2, error_message = $3, \
                 next_attempt_at = now() + $4 * INTERVAL '1 second', \
                 dispatched_at = now() \
             WHERE id = $5",
        )
        .bind(status)
        .bind(http_status as i32)
        .bind(error)
        .bind(secs)
        .bind(delivery_id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            "UPDATE fluxbase_internal.event_deliveries \
             SET status = $1, response_status = $2, error_message = $3, \
                 dispatched_at = now(), next_attempt_at = NULL \
             WHERE id = $4",
        )
        .bind(status)
        .bind(http_status as i32)
        .bind(error)
        .bind(delivery_id)
        .execute(pool)
        .await
    };
}

/// Mark event delivered only when no 'pending' or retryable 'failed' deliveries remain.
async fn check_and_mark_event_delivered(pool: &PgPool, event_id: Uuid) {
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM fluxbase_internal.event_deliveries \
         WHERE event_id = $1 AND status IN ('pending', 'failed')",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .unwrap_or(1); // assume pending on error

    if pending == 0 {
        mark_delivered(pool, event_id).await;
    }
}

async fn mark_delivered(pool: &PgPool, event_id: Uuid) {
    let _ = sqlx::query(
        "UPDATE fluxbase_internal.events SET delivered_at = now() WHERE id = $1",
    )
    .bind(event_id)
    .execute(pool)
    .await;
}

async fn load_matching_subscriptions(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    event_type: &str,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    let table_wildcard = event_type
        .split_once('.')
        .map(|(t, _)| format!("{}.*", t))
        .unwrap_or_else(|| "*".to_string());

    sqlx::query(
        "SELECT id, target_type, target_config, max_attempts \
         FROM fluxbase_internal.event_subscriptions \
         WHERE tenant_id = $1 AND project_id = $2 \
           AND enabled = TRUE \
           AND event_pattern IN ($3, $4, '*')",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(event_type)
    .bind(&table_wildcard)
    .fetch_all(pool)
    .await
}

/// Exponential back-off in seconds: 2, 4, 30, 300, 1800.
fn retry_delay_secs(attempt: i32) -> i64 {
    match attempt {
        1 => 2,
        2 => 4,
        3 => 30,
        4 => 300,
        _ => 1800,
    }
}
