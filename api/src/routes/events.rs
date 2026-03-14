use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResponse},
    types::context::RequestContext,
    validation::PaginationQuery,
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

#[derive(sqlx::FromRow, Serialize)]
pub struct EventRow {
    pub id: Uuid,
    pub event_type: String,
    pub table_name: String,
    pub record_id: Option<String>,
    pub operation: String,
    pub payload: Value,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct EventSubscriptionRow {
    pub id: Uuid,
    pub event_pattern: String,
    pub target_type: String,
    pub target_config: Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct PublishEventPayload {
    pub event: String,
    pub payload: Option<Value>,
}

#[derive(Deserialize)]
pub struct CreateSubscriptionPayload {
    pub event_pattern: String,
    pub target_type: String,
    pub target_config: Option<Value>,
}

pub async fn publish_event(
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<PublishEventPayload>,
) -> ApiResult<EventRow> {
    let parts: Vec<&str> = payload.event.splitn(2, '.').collect();
    let table_name = parts[0];
    let operation = parts.get(1).unwrap_or(&"custom").to_string();

    let row = sqlx::query_as::<_, EventRow>(
        "INSERT INTO fluxbase_internal.events \
         (event_type, table_name, operation, payload) \
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(&payload.event)
    .bind(table_name)
    .bind(&operation)
    .bind(payload.payload.unwrap_or(Value::Object(Default::default())))
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

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
