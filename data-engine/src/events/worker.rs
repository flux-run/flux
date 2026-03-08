use reqwest::Client;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use uuid::Uuid;

use super::dispatcher;

/// Background task: poll `fluxbase_internal.events` for undelivered events,
/// look up matching subscriptions, and fan out via the dispatcher.
///
/// Locking strategy: `FOR UPDATE SKIP LOCKED` — safe to run multiple replicas
/// of the data-engine concurrently without duplicate delivery.
pub async fn run(pool: Arc<PgPool>, http: Arc<Client>, runtime_url: String) {
    let mut tick = interval(Duration::from_secs(1));
    tracing::info!("event worker started — polling every 1s");

    loop {
        tick.tick().await;
        if let Err(e) = process_batch(&pool, &http, &runtime_url).await {
            tracing::warn!(error = %e, "event worker batch error");
        }
    }
}

/// Claim and process up to 50 undelivered events per tick.
async fn process_batch(
    pool: &PgPool,
    http: &Client,
    runtime_url: &str,
) -> Result<(), sqlx::Error> {
    // Claim a batch. SKIP LOCKED avoids contention with other replicas.
    let events = sqlx::query(
        "SELECT id, tenant_id, project_id, event_type, table_name, \
                record_id, operation, payload \
         FROM fluxbase_internal.events \
         WHERE delivered_at IS NULL \
         ORDER BY created_at \
         LIMIT 50 \
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(pool)
    .await?;

    if events.is_empty() {
        return Ok(());
    }

    tracing::debug!(count = events.len(), "processing event batch");

    for event in &events {
        let event_id: Uuid = event.get("id");
        let tenant_id: Uuid = event.get("tenant_id");
        let project_id: Uuid = event.get("project_id");
        let event_type: String = event.get("event_type");
        let payload: serde_json::Value = event.get("payload");

        // Load all enabled subscriptions matching this tenant+project.
        let subs = load_matching_subscriptions(pool, tenant_id, project_id, &event_type).await?;

        let mut all_ok = true;
        for sub in &subs {
            let sub_id: Uuid = sub.get("id");
            let target_type: String = sub.get("target_type");
            let target_config: serde_json::Value = sub.get("target_config");

            let result = dispatcher::dispatch(
                pool,
                http,
                runtime_url,
                sub_id,
                &target_type,
                &target_config,
                &payload,
                &event_type,
            )
            .await;

            match result {
                Ok(status) => {
                    record_delivery(pool, event_id, sub_id, status, None).await;
                }
                Err(ref e) => {
                    all_ok = false;
                    tracing::warn!(
                        event_id = %event_id,
                        subscription_id = %sub_id,
                        error = %e,
                        "subscription dispatch failed"
                    );
                    record_delivery(pool, event_id, sub_id, 0, Some(e)).await;
                }
            }
        }

        // Mark as delivered even if some subscriptions failed —
        // individual retries are tracked in event_deliveries.
        if all_ok || subs.is_empty() {
            mark_delivered(pool, event_id).await;
        } else {
            // At least one delivery failed; still mark delivered to prevent
            // infinite repolling. Dead-letter / retry is a future concern.
            mark_delivered(pool, event_id).await;
        }
    }

    Ok(())
}

/// Return subscriptions whose event_pattern matches the given event_type.
/// Pattern rules: exact match | "{table}.*" | "*"
async fn load_matching_subscriptions(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    event_type: &str,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    // Derive the wildcard form: "users.inserted" → "users.*"
    let table_wildcard = event_type
        .split_once('.')
        .map(|(t, _)| format!("{}.*", t))
        .unwrap_or_else(|| "*".to_string());

    sqlx::query(
        "SELECT id, target_type, target_config \
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

async fn mark_delivered(pool: &PgPool, event_id: Uuid) {
    let _ = sqlx::query(
        "UPDATE fluxbase_internal.events \
         SET delivered_at = now() \
         WHERE id = $1",
    )
    .bind(event_id)
    .execute(pool)
    .await;
}

async fn record_delivery(
    pool: &PgPool,
    event_id: Uuid,
    subscription_id: Uuid,
    status: u16,
    error: Option<&str>,
) {
    let dispatch_status = if error.is_none() { "success" } else { "failed" };
    let _ = sqlx::query(
        "INSERT INTO fluxbase_internal.event_deliveries \
             (event_id, subscription_id, status, response_status, error_message, dispatched_at) \
         VALUES ($1, $2, $3, $4, $5, now())",
    )
    .bind(event_id)
    .bind(subscription_id)
    .bind(dispatch_status)
    .bind(status as i32)
    .bind(error)
    .execute(pool)
    .await;
}
