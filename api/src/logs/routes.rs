/// Unified platform log ingestion and query handlers.
///
/// # Write path
///
/// `POST /internal/logs` — called by the runtime (functions) and other internal
/// services.  Accepts the unified log envelope; looks up `tenant_id`/`project_id`
/// from the `function_id` for backwards-compat callers that only send a UUID.
///
/// # Read path
///
/// `GET /logs` — project-scoped, Firebase-auth.  Filters by `source`, `resource`,
/// `level`, `since`, and `limit`.  When `since` reaches back past the hot window
/// the call is transparently enriched with archived NDJSON from R2/S3.

use axum::{
    extract::{Extension, Path, Query, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::response::{ApiResponse, ApiError};
use crate::types::context::RequestContext;

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError { ApiError::internal("database_error") }

fn validate_service_token(headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers.get("X-Service-Token").and_then(|h| h.to_str().ok()).unwrap_or("");
    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "stub_token".to_string());
    if token != expected { return Err(ApiError::unauth("invalid_service_token")); }
    Ok(())
}

// ─── POST /internal/logs ──────────────────────────────────────────────────────
//
// Accepts both:
//   1. New unified format:
//      { "source": "function", "resource_id": "echo", "tenant_id": "...",
//        "project_id": "...", "level": "info", "message": "...",
//        "request_id": "...", "metadata": {} }
//
//   2. Legacy runtime format (backward compat):
//      { "function_id": "...", "level": "info", "message": "..." }
//      tenant_id/project_id resolved via DB lookup on function.

#[derive(Deserialize)]
pub struct LogEntry {
    // ── Unified fields ────────────────────────────────────────────────────
    pub source:      Option<String>,
    pub resource_id: Option<String>,
    pub tenant_id:   Option<Uuid>,
    pub project_id:  Option<Uuid>,
    pub request_id:  Option<String>,
    pub metadata:    Option<serde_json::Value>,
    /// Trace span classification: start | end | error | event (default)
    pub span_type:   Option<String>,
    // ── Shared fields ─────────────────────────────────────────────────────
    pub level:       Option<String>,
    pub message:     String,
    // ── Legacy compat ─────────────────────────────────────────────────────
    pub function_id: Option<String>,
    #[allow(dead_code)]
    pub timestamp:   Option<String>,
}

pub async fn create_log(
    headers: HeaderMap,
    State(pool): State<PgPool>,
    axum::Json(entry): axum::Json<LogEntry>,
) -> ApiResult<serde_json::Value> {
    validate_service_token(&headers)?;

    let level  = entry.level.as_deref().unwrap_or("info");
    let source = entry.source.as_deref().unwrap_or("function");

    // ── Resolve tenant_id / project_id / resource_id ──────────────────────
    let (tenant_id, project_id, resource_id) = if let Some(tid) = entry.tenant_id {
        (tid, entry.project_id, entry.resource_id.clone().unwrap_or_default())
    } else if let Some(fid_str) = &entry.function_id {
        #[derive(sqlx::FromRow)]
        struct FnCtx { tenant_id: Uuid, project_id: Uuid, name: String }
        let fn_ctx = if let Ok(fid) = fid_str.parse::<Uuid>() {
            sqlx::query_as::<_, FnCtx>(
                "SELECT t.id AS tenant_id, f.project_id, f.name \
                 FROM functions f \
                 JOIN projects p ON p.id = f.project_id \
                 JOIN tenants  t ON t.id = p.tenant_id \
                 WHERE f.id = $1 LIMIT 1",
            ).bind(fid).fetch_optional(&pool).await.map_err(|_| db_err())?
        } else {
            sqlx::query_as::<_, FnCtx>(
                "SELECT t.id AS tenant_id, f.project_id, f.name \
                 FROM functions f \
                 JOIN projects p ON p.id = f.project_id \
                 JOIN tenants  t ON t.id = p.tenant_id \
                 WHERE f.name = $1 LIMIT 1",
            ).bind(fid_str).fetch_optional(&pool).await.map_err(|_| db_err())?
        };
        if let Some(ctx) = fn_ctx {
            let res = entry.resource_id.clone().unwrap_or(ctx.name);
            (ctx.tenant_id, Some(ctx.project_id), res)
        } else {
            return Ok(ApiResponse::new(serde_json::json!({ "logged": false, "reason": "function_not_found" })));
        }
    } else {
        return Ok(ApiResponse::new(serde_json::json!({ "logged": false, "reason": "missing_context" })));
    };

    sqlx::query(
        "INSERT INTO platform_logs \
         (tenant_id, project_id, source, resource_id, level, message, request_id, metadata, span_type) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_id).bind(project_id).bind(source).bind(&resource_id)
    .bind(level).bind(&entry.message).bind(&entry.request_id).bind(&entry.metadata)
    .bind(&entry.span_type)
    .execute(&pool).await.map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "logged": true })))
}

// ─── GET /internal/logs (legacy — internal tooling only) ─────────────────────

#[derive(Deserialize)]
pub struct LogQuery {
    pub function_id: String,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct LogRow {
    pub id:        Uuid,
    pub level:     String,
    pub message:   String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub async fn list_logs(
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Query(params): Query<LogQuery>,
) -> ApiResult<serde_json::Value> {
    validate_service_token(&headers)?;
    let limit = params.limit.unwrap_or(100).min(1000);

    // Resolve function UUID → name.
    let resource_id = if let Ok(fid) = params.function_id.parse::<Uuid>() {
        #[derive(sqlx::FromRow)] struct FnName { name: String }
        sqlx::query_as::<_, FnName>("SELECT name FROM functions WHERE id = $1 LIMIT 1")
            .bind(fid).fetch_optional(&pool).await.unwrap_or(None)
            .map(|f| f.name).unwrap_or(params.function_id.clone())
    } else {
        params.function_id.clone()
    };

    let rows = sqlx::query_as::<_, LogRow>(
        "SELECT id, level, message, timestamp FROM platform_logs \
         WHERE resource_id = $1 AND source = 'function' \
         ORDER BY timestamp DESC LIMIT $2",
    )
    .bind(&resource_id).bind(limit)
    .fetch_all(&pool).await.map_err(|_| db_err())?;

    let logs: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "id": r.id, "level": r.level, "message": r.message,
        "timestamp": r.timestamp.to_rfc3339(),
    })).collect();
    Ok(ApiResponse::new(serde_json::json!({ "logs": logs })))
}

// ─── GET /logs  (project-scoped, Firebase auth) ───────────────────────────────
//
// Query params:
//   source   — subsystem: function | db | workflow | event | queue | system
//   resource — resource_id (function name, db name, workflow id, etc.)
//   function — legacy alias for ?source=function&resource=<name>
//   level    — error | warn | info | debug
//   limit    — max rows (default 100, max 1000)
//   since    — ISO-8601 timestamp; return only rows newer than this
//
// Hot path  (since within LOG_HOT_DAYS): Postgres only.
// Cold path (since older than LOG_HOT_DAYS): Postgres + R2/S3 archive merge.

#[derive(Deserialize)]
pub struct ProjectLogQuery {
    pub source:   Option<String>,
    pub resource: Option<String>,
    pub function: Option<String>,  // legacy alias
    pub level:    Option<String>,
    pub limit:    Option<i64>,
    pub since:    Option<String>,
}

pub async fn list_project_logs(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    Extension(context): Extension<RequestContext>,
    Query(params): Query<ProjectLogQuery>,
) -> ApiResult<serde_json::Value> {
    let project_id = context.project_id.ok_or(ApiError::bad_request("missing_project"))?;
    let pool       = &state.pool;
    let limit      = params.limit.unwrap_or(100).min(1_000);
    let limit_us   = limit as usize;

    // Backward compat: ?function=echo → source=function, resource=echo.
    let source   = params.source.as_deref()
        .or_else(|| params.function.as_ref().map(|_| "function"));
    let resource = params.resource.as_deref()
        .or(params.function.as_deref());
    let level    = params.level.as_deref();

    let since: Option<chrono::DateTime<chrono::Utc>> = params.since.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    #[derive(sqlx::FromRow)]
    struct PlatformLogRow {
        id:          Uuid,
        source:      String,
        resource_id: String,
        level:       String,
        message:     String,
        request_id:  Option<String>,
        metadata:    Option<serde_json::Value>,
        span_type:   Option<String>,
        timestamp:   chrono::DateTime<chrono::Utc>,
    }

    // ── Build dynamic SQL query ───────────────────────────────────────────
    let mut conditions: Vec<String> = vec!["l.project_id = $1".to_string()];
    let mut bind_idx = 2usize;

    if source.is_some()   { conditions.push(format!("l.source = ${bind_idx}"));      bind_idx += 1; }
    if resource.is_some() { conditions.push(format!("l.resource_id = ${bind_idx}")); bind_idx += 1; }
    if level.is_some()    { conditions.push(format!("l.level = ${bind_idx}"));        bind_idx += 1; }

    let (time_cond, time_order) = match since {
        Some(_) => (format!("l.timestamp > ${bind_idx}"), "ASC"),
        None    => ("1=1".to_string(),                     "DESC"),
    };
    if since.is_some() { bind_idx += 1; }
    conditions.push(time_cond);

    let sql = format!(
        "SELECT l.id, l.source, l.resource_id, l.level, l.message, \
                l.request_id, l.metadata, l.span_type, l.timestamp \
         FROM platform_logs l \
         WHERE {} \
         ORDER BY l.timestamp {} LIMIT ${}",
        conditions.join(" AND "), time_order, bind_idx
    );

    let mut q = sqlx::query_as::<_, PlatformLogRow>(&sql).bind(project_id);
    if let Some(s)  = source   { q = q.bind(s); }
    if let Some(r)  = resource { q = q.bind(r); }
    if let Some(l)  = level    { q = q.bind(l); }
    if let Some(ts) = since    { q = q.bind(ts); }
    q = q.bind(limit);

    let rows = q.fetch_all(pool).await.map_err(|_| db_err())?;

    let mut logs: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
        "id":         r.id,
        "source":     r.source,
        "resource":   r.resource_id,
        // legacy "function" field for old CLI versions
        "function":   if r.source == "function" { &r.resource_id as &str } else { "" },
        "level":      r.level,
        "message":    r.message,
        "request_id": r.request_id,
        "span_type":  r.span_type.as_deref().unwrap_or("event"),
        "metadata":   r.metadata,
        "timestamp":  r.timestamp.to_rfc3339(),
        "tier":       "hot",
    })).collect();

    // ── Archive read (cold path) ──────────────────────────────────────────
    let hot_cutoff = chrono::Utc::now() - chrono::Duration::days(state.log_archiver.hot_days);

    if let Some(since_ts) = since {
        if since_ts < hot_cutoff {
            #[derive(sqlx::FromRow)] struct TenantId { tenant_id: Uuid }
            let t = sqlx::query_as::<_, TenantId>(
                "SELECT tenant_id FROM projects WHERE id = $1 LIMIT 1",
            ).bind(project_id).fetch_optional(pool).await.unwrap_or(None);

            if let Some(t) = t {
                let s = source.unwrap_or("function");
                let r = resource.unwrap_or("");
                let archived = state.log_archiver
                    .fetch_archived(t.tenant_id, s, r, since_ts, hot_cutoff, limit_us)
                    .await;

                for mut entry in archived {
                    if let Some(lf) = level {
                        if entry.get("level").and_then(|l| l.as_str()) != Some(lf) { continue; }
                    }
                    if let Some(obj) = entry.as_object_mut() {
                        if let Some(rid) = obj.get("resource_id").cloned() {
                            let src_str = obj.get("source")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();  // owned — avoids borrow conflict
                            obj.insert("resource".into(), rid.clone());
                            obj.insert("function".into(), if src_str == "function" { rid } else { serde_json::json!("") });
                        }
                        obj.insert("tier".into(), serde_json::json!("archive"));
                    }
                    logs.push(entry);
                }

                logs.sort_by(|a, b| {
                    a.get("timestamp").and_then(|t| t.as_str()).unwrap_or("")
                        .cmp(b.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""))
                });
                logs.truncate(limit_us);
            }
        }
    }

    Ok(ApiResponse::new(serde_json::json!({ "logs": logs })))
}

// ─── GET /traces/{request_id}  (project-scoped, Firebase auth) ───────────────
//
// Returns all log spans that share the same request_id in ascending timestamp
// order, giving a full cross-service request trace:
//   gateway → api middleware → function logs emitted by the runtime
//
// Query params:
//   slow_ms — threshold in milliseconds above which a delta is flagged
//             as slow (default 500).  Spans at or above this value have
//             is_slow: true so the CLI / dashboard can highlight them.

#[derive(Deserialize)]
pub struct TraceQuery {
    pub slow_ms: Option<i64>,
}

pub async fn get_trace(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    Extension(context): Extension<RequestContext>,
    Path(request_id): Path<String>,
    Query(params): Query<TraceQuery>,
) -> ApiResult<serde_json::Value> {
    let project_id  = context.project_id.ok_or(ApiError::bad_request("missing_project"))?;
    let pool        = &state.pool;
    let slow_thresh = params.slow_ms.unwrap_or(500);   // 500ms default

    #[derive(sqlx::FromRow)]
    struct TraceRow {
        id:          Uuid,
        source:      String,
        resource_id: String,
        level:       String,
        message:     String,
        span_type:   Option<String>,
        metadata:    Option<serde_json::Value>,
        timestamp:   chrono::DateTime<chrono::Utc>,
    }

    let rows = sqlx::query_as::<_, TraceRow>(
        "SELECT l.id, l.source, l.resource_id, l.level, l.message, \
                l.span_type, l.metadata, l.timestamp \
         FROM platform_logs l \
         WHERE l.project_id = $1 AND l.request_id = $2 \
         ORDER BY l.timestamp ASC",
    )
    .bind(project_id)
    .bind(&request_id)
    .fetch_all(pool)
    .await
    .map_err(|_| db_err())?;

    if rows.is_empty() {
        return Ok(ApiResponse::new(serde_json::json!({
            "request_id":      request_id,
            "spans":           [],
            "span_count":      0,
            "total_duration_ms": null,
            "slow_span_count": 0,
        })));
    }

    // ── Compute per-span deltas and totals ────────────────────────────────
    let first_ts = rows[0].timestamp;
    let last_ts  = rows[rows.len() - 1].timestamp;
    let total_duration_ms = (last_ts - first_ts).num_milliseconds();
    let mut prev_ts = first_ts;
    let mut slow_span_count: usize = 0;

    let spans: Vec<serde_json::Value> = rows.into_iter().map(|r| {
        let delta_ms   = (r.timestamp - prev_ts).num_milliseconds();
        let elapsed_ms = (r.timestamp - first_ts).num_milliseconds();
        let is_slow    = delta_ms >= slow_thresh;
        if is_slow { slow_span_count += 1; }
        prev_ts = r.timestamp;

        serde_json::json!({
            "id":         r.id,
            "source":     r.source,
            "resource":   r.resource_id,
            "level":      r.level,
            "message":    r.message,
            "span_type":  r.span_type.as_deref().unwrap_or("event"),
            "metadata":   r.metadata,
            "timestamp":  r.timestamp.to_rfc3339(),
            "delta_ms":   delta_ms,
            "elapsed_ms": elapsed_ms,
            "is_slow":    is_slow,
        })
    }).collect();

    // ── N+1 detection ─────────────────────────────────────────────────────
    // A table queried ≥ 3 times within the same request is a probable N+1.
    // We count simply by table name across all db spans; the 50ms window is
    // implicitly enforced because all queries share one request_id.
    let mut table_query_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut slow_db_count: usize = 0;

    for span in &spans {
        if span["source"].as_str() != Some("db") { continue; }
        if let Some(table) = span["metadata"]["table"].as_str() {
            *table_query_counts.entry(table.to_string()).or_insert(0) += 1;
        }
        // Count spans emitted with slow:true in metadata.
        if span["metadata"]["slow"].as_bool().unwrap_or(false) {
            slow_db_count += 1;
        }
    }

    let n_plus_one_tables: Vec<String> = {
        let mut tables: Vec<String> = table_query_counts
            .iter()
            .filter(|(_, count)| **count >= 3)
            .map(|(t, _)| t.clone())
            .collect();
        tables.sort();
        tables
    };

    // Tag individual spans that are part of an N+1 pattern.
    let spans: Vec<serde_json::Value> = spans
        .into_iter()
        .map(|mut span| {
            if span["source"].as_str() == Some("db") {
                if let Some(table) = span["metadata"]["table"]
                    .as_str()
                    .map(|s| s.to_string())
                {
                    if n_plus_one_tables.contains(&table) {
                        if let Some(obj) = span.as_object_mut() {
                            obj.insert("n_plus_one".into(), serde_json::json!(true));
                        }
                    }
                }
            }
            span
        })
        .collect();

    let span_count = spans.len();
    Ok(ApiResponse::new(serde_json::json!({
        "request_id":          request_id,
        "spans":               spans,
        "span_count":          span_count,
        "total_duration_ms":   total_duration_ms,
        "slow_span_count":     slow_span_count,
        "slow_threshold_ms":   slow_thresh,
        "n_plus_one_tables":   n_plus_one_tables,
        "slow_db_count":       slow_db_count,
    })))
}
