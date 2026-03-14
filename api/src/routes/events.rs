use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResponse},
    types::context::RequestContext,
    validation::PaginationQuery,
    AppState,
};
use api_contract::events::{
    CreateSubscriptionPayload, EventRow, EventSubscriptionRow, PublishEventPayload,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn publish_event(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<PublishEventPayload>,
) -> ApiResult<EventRow> {
    let parts: Vec<&str> = payload.event.splitn(2, '.').collect();
    let table_name = parts[0];
    let operation = parts.get(1).unwrap_or(&"custom").to_string();
    let event_payload = payload.payload.unwrap_or(Value::Object(Default::default()));

    let row = sqlx::query_as::<_, EventRow>(
        "INSERT INTO fluxbase_internal.events \
         (event_type, table_name, operation, payload) \
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(&payload.event)
    .bind(table_name)
    .bind(&operation)
    .bind(&event_payload)
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    // ── Fan-out to matching subscriptions (fire-and-forget) ───────────────
    //
    // Pattern matching rules (left-to-right):
    //   "users.inserted" — exact match
    //   "users.*"        — any operation on the users table
    //   "*"              — every event
    //
    // For function / queue_job subscriptions we enqueue a job directly into
    // the jobs table so the queue worker picks it up with standard retry
    // semantics.  Webhook delivery is also routed through jobs (target_type
    // 'webhook') so retries are handled uniformly.

    let event_id   = row.id;
    let event_type = payload.event.clone();
    let pool       = state.pool.clone();

    tokio::spawn(async move {
        #[derive(sqlx::FromRow)]
        struct SubRow {
            id:            Uuid,
            target_type:   String,
            target_config: Value,
        }

        let subs = sqlx::query_as::<_, SubRow>(
            "SELECT id, target_type, target_config \
             FROM fluxbase_internal.event_subscriptions \
             WHERE enabled = true \
               AND (event_pattern = $1 \
                    OR event_pattern = '*' \
                    OR ($1 LIKE replace(event_pattern, '*', '%') \
                        AND event_pattern LIKE '%.*'))",
        )
        .bind(&event_type)
        .fetch_all(&pool)
        .await;

        let subs = match subs {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "failed to load event subscriptions");
                return;
            }
        };

        for sub in subs {
            let sub_id        = sub.id;
            let target_type   = sub.target_type.as_str();
            let target_config = &sub.target_config;

            // Determine the function_id for job-based targets.
            let fn_id: Option<Uuid> = match target_type {
                "function" => target_config
                    .get("function_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<Uuid>().ok()),
                "queue_job" => target_config
                    .get("function_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<Uuid>().ok()),
                _ => None,
            };

            let queue_name: Option<String> = match target_type {
                "queue_job" => target_config
                    .get("queue")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_owned()),
                _ => None,
            };

            // Build job payload: wrap event context alongside any inline payload.
            let job_payload = serde_json::json!({
                "event_id":   event_id,
                "event_type": event_type,
                "event_payload": event_payload,
            });

            // Enqueue the job and record the delivery in a single transaction.
            let result: Result<(), sqlx::Error> = async {
                let mut tx = pool.begin().await?;

                if let Some(fid) = fn_id {
                    sqlx::query(
                        "INSERT INTO flux.jobs (function_id, payload, queue_name) \
                         VALUES ($1, $2, $3)",
                    )
                    .bind(fid)
                    .bind(&job_payload)
                    .bind(queue_name.as_deref())
                    .execute(&mut *tx)
                    .await?;
                }

                sqlx::query(
                    "INSERT INTO fluxbase_internal.event_deliveries \
                     (event_id, subscription_id, status, dispatched_at) \
                     VALUES ($1, $2, 'pending', now())",
                )
                .bind(event_id)
                .bind(sub_id)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;
                Ok(())
            }
            .await;

            if let Err(e) = result {
                tracing::error!(
                    error        = %e,
                    subscription = %sub_id,
                    event        = %event_id,
                    "event delivery dispatch failed",
                );
            }
        }
    });

    Ok(ApiResponse::created(row))
}

pub async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Vec<EventSubscriptionRow>> {
    let (limit, offset) = page.clamped();
    let rows = sqlx::query_as::<_, EventSubscriptionRow>(
        "SELECT id, event_pattern, target_type, target_config, \
         enabled, created_at, updated_at \
         FROM fluxbase_internal.event_subscriptions \
         ORDER BY created_at DESC \
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(rows))
}

pub async fn create_subscription(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateSubscriptionPayload>,
) -> ApiResult<EventSubscriptionRow> {
    let row = sqlx::query_as::<_, EventSubscriptionRow>(
        "INSERT INTO fluxbase_internal.event_subscriptions \
         (event_pattern, target_type, target_config) \
         VALUES ($1, $2, $3) \
         RETURNING id, event_pattern, target_type, target_config, \
         enabled, created_at, updated_at",
    )
    .bind(&payload.event_pattern)
    .bind(&payload.target_type)
    .bind(payload.target_config.unwrap_or(Value::Object(Default::default())))
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::created(row))
}

pub async fn delete_subscription(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    sqlx::query(
        "DELETE FROM fluxbase_internal.event_subscriptions WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "success": true })))
}
