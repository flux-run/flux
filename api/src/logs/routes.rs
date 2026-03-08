use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::response::{ApiResponse, ApiError};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError {
    ApiError::internal("database_error")
}

fn validate_service_token(headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers
        .get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "stub_token".to_string());
    if token != expected {
        return Err(ApiError::unauth("invalid_service_token"));
    }
    Ok(())
}

// ── POST /internal/logs ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogEntry {
    pub function_id: String,
    pub level: Option<String>,
    pub message: String,
    pub timestamp: Option<String>,
}

pub async fn create_log(
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Json(entry): Json<LogEntry>,
) -> ApiResult<serde_json::Value> {
    validate_service_token(&headers)?;

    // Accept both UUID and function name — try UUID first
    let function_id: Option<Uuid> = entry.function_id.parse().ok();
    let level = entry.level.as_deref().unwrap_or("info");

    if let Some(fid) = function_id {
        sqlx::query(
            "INSERT INTO function_logs (function_id, level, message) VALUES ($1, $2, $3)"
        )
        .bind(fid)
        .bind(level)
        .bind(&entry.message)
        .execute(&pool)
        .await
        .map_err(|_| db_err())?;
    } else {
        // Look up function by name (across all tenants for internal use)
        #[derive(sqlx::FromRow)]
        struct FnId { id: Uuid }
        let fn_row = sqlx::query_as::<_, FnId>(
            "SELECT id FROM functions WHERE name = $1 LIMIT 1"
        )
        .bind(&entry.function_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| db_err())?;

        if let Some(f) = fn_row {
            sqlx::query(
                "INSERT INTO function_logs (function_id, level, message) VALUES ($1, $2, $3)"
            )
            .bind(f.id)
            .bind(level)
            .bind(&entry.message)
            .execute(&pool)
            .await
            .map_err(|_| db_err())?;
        }
        // Silently ignore if function not found (log best-effort)
    }

    Ok(ApiResponse::new(serde_json::json!({ "logged": true })))
}

// ── GET /internal/logs?function_id= ───────────────────────────────────────

#[derive(Deserialize)]
pub struct LogQuery {
    pub function_id: String,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct LogRow {
    pub id: Uuid,
    pub level: String,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub async fn list_logs(
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Query(params): Query<LogQuery>,
) -> ApiResult<serde_json::Value> {
    validate_service_token(&headers)?;

    let limit = params.limit.unwrap_or(100).min(1000);

    let rows = if let Ok(fid) = params.function_id.parse::<Uuid>() {
        sqlx::query_as::<_, LogRow>(
            "SELECT id, level, message, timestamp FROM function_logs \
             WHERE function_id = $1 ORDER BY timestamp DESC LIMIT $2"
        )
        .bind(fid)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .map_err(|_| db_err())?
    } else {
        sqlx::query_as::<_, LogRow>(
            "SELECT l.id, l.level, l.message, l.timestamp FROM function_logs l \
             JOIN functions f ON f.id = l.function_id \
             WHERE f.name = $1 ORDER BY l.timestamp DESC LIMIT $2"
        )
        .bind(&params.function_id)
        .bind(limit)
        .fetch_all(&pool)
        .await
        .map_err(|_| db_err())?
    };

    let logs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "id": r.id,
        "level": r.level,
        "message": r.message,
        "timestamp": r.timestamp.to_rfc3339(),
    })).collect();

    Ok(ApiResponse::new(serde_json::json!({ "logs": logs })))
}
