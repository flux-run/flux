use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::response::{ApiResponse, ApiError};
use crate::types::context::RequestContext;

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

// ── GET /logs  (project-scoped, Firebase auth) ────────────────────────────
//
// Query params:
//   function  — optional function name filter
//   limit     — max rows (default 100, max 1000)
//   since     — ISO-8601 timestamp; return only rows newer than this
//
// Hot path (since within LOG_HOT_DAYS): Postgres only.
// Cold path (since older than LOG_HOT_DAYS or no since but old data needed):
//   Postgres results are merged with archived NDJSON files from R2/S3.
//
// Used by `flux logs` (fetch) and `flux logs --follow` (polled).

#[derive(Deserialize)]
pub struct ProjectLogQuery {
    pub function: Option<String>,
    pub limit: Option<i64>,
    pub since: Option<String>,   // ISO-8601 e.g. "2026-03-09T10:00:00Z"
}

pub async fn list_project_logs(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    Extension(context): Extension<RequestContext>,
    Query(params): Query<ProjectLogQuery>,
) -> ApiResult<serde_json::Value> {
    let project_id = context
        .project_id
        .ok_or(ApiError::bad_request("missing_project"))?;

    let pool  = &state.pool;
    let limit = params.limit.unwrap_or(100).min(1_000) as usize;
    let limit_i64 = limit as i64;

    // Parse optional `since` timestamp.
    let since: Option<chrono::DateTime<chrono::Utc>> = params
        .since
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    #[derive(sqlx::FromRow)]
    struct ProjectLogRow {
        id:            Uuid,
        function_name: String,
        level:         String,
        message:       String,
        timestamp:     chrono::DateTime<chrono::Utc>,
    }

    // ── Hot tier: Postgres ─────────────────────────────────────────────────
    let rows: Vec<ProjectLogRow> = match (params.function.as_deref(), since) {
        (Some(fn_name), Some(since_ts)) => sqlx::query_as(
            "SELECT l.id, f.name as function_name, l.level, l.message, l.timestamp \
             FROM function_logs l \
             JOIN functions f ON f.id = l.function_id \
             WHERE f.project_id = $1 AND f.name = $2 AND l.timestamp > $3 \
             ORDER BY l.timestamp ASC LIMIT $4",
        )
        .bind(project_id).bind(fn_name).bind(since_ts).bind(limit_i64)
        .fetch_all(pool).await.map_err(|_| db_err())?,

        (Some(fn_name), None) => sqlx::query_as(
            "SELECT l.id, f.name as function_name, l.level, l.message, l.timestamp \
             FROM function_logs l \
             JOIN functions f ON f.id = l.function_id \
             WHERE f.project_id = $1 AND f.name = $2 \
             ORDER BY l.timestamp DESC LIMIT $3",
        )
        .bind(project_id).bind(fn_name).bind(limit_i64)
        .fetch_all(pool).await.map_err(|_| db_err())?,

        (None, Some(since_ts)) => sqlx::query_as(
            "SELECT l.id, f.name as function_name, l.level, l.message, l.timestamp \
             FROM function_logs l \
             JOIN functions f ON f.id = l.function_id \
             WHERE f.project_id = $1 AND l.timestamp > $2 \
             ORDER BY l.timestamp ASC LIMIT $3",
        )
        .bind(project_id).bind(since_ts).bind(limit_i64)
        .fetch_all(pool).await.map_err(|_| db_err())?,

        (None, None) => sqlx::query_as(
            "SELECT l.id, f.name as function_name, l.level, l.message, l.timestamp \
             FROM function_logs l \
             JOIN functions f ON f.id = l.function_id \
             WHERE f.project_id = $1 \
             ORDER BY l.timestamp DESC LIMIT $2",
        )
        .bind(project_id).bind(limit_i64)
        .fetch_all(pool).await.map_err(|_| db_err())?,
    };

    let mut logs: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
        "id":        r.id,
        "function":  r.function_name,
        "level":     r.level,
        "message":   r.message,
        "timestamp": r.timestamp.to_rfc3339(),
        "source":    "hot",
    })).collect();

    // ── Cold tier: R2/S3 archive ───────────────────────────────────────────
    //
    // Only triggered when the caller explicitly reaches back past the hot
    // window (i.e. `since` is older than `NOW() - LOG_HOT_DAYS`).
    // Typical `flux logs` calls (showing the last N lines) never hit this path.
    let hot_cutoff = chrono::Utc::now()
        - chrono::Duration::days(state.log_archiver.hot_days);

    if let Some(since_ts) = since {
        if since_ts < hot_cutoff {
            // Resolve function UUIDs (needed to address archive objects by key).
            #[derive(sqlx::FromRow)]
            struct FnInfo { id: Uuid, name: String }

            let fns: Vec<FnInfo> = if let Some(fn_name) = params.function.as_deref() {
                sqlx::query_as(
                    "SELECT id, name FROM functions WHERE project_id = $1 AND name = $2",
                )
                .bind(project_id).bind(fn_name)
                .fetch_all(pool).await.unwrap_or_default()
            } else {
                sqlx::query_as(
                    "SELECT id, name FROM functions WHERE project_id = $1",
                )
                .bind(project_id)
                .fetch_all(pool).await.unwrap_or_default()
            };

            // Remaining budget for archive rows (always fetch up to `limit`
            // archive rows per function so callers get full coverage).
            let per_fn_limit = limit.max(1);

            for fn_info in &fns {
                // Fetch only the archived portion: [since_ts, hot_cutoff).
                let archived = state.log_archiver
                    .fetch_archived(fn_info.id, since_ts, hot_cutoff, per_fn_limit)
                    .await;

                for mut entry in archived {
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("function".into(), serde_json::json!(fn_info.name));
                        obj.insert("source".into(),   serde_json::json!("archive"));
                    }
                    logs.push(entry);
                }
            }

            // Re-sort the merged set by timestamp ascending and cap at limit.
            logs.sort_by(|a, b| {
                a.get("timestamp").and_then(|t| t.as_str()).unwrap_or("")
                    .cmp(b.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""))
            });
            logs.truncate(limit);
        }
    }

    Ok(ApiResponse::new(serde_json::json!({ "logs": logs })))
}
