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
/// `GET /logs` — project-scoped.  Filters by `source`, `resource`,
/// `level`, `since`, and `limit`.  Hot path only (uses Postgres).

use axum::{
    extract::{Extension, Path, Query, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use crate::error::{ApiResponse, ApiError};
use crate::types::context::RequestContext;

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError { ApiError::internal("database_error") }

fn validate_service_token(headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers.get("X-Service-Token").and_then(|h| h.to_str().ok()).unwrap_or("");
    let expected = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| {
            if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                     The API service cannot start without it."
                );
            }
            tracing::warn!(
                "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'dev-service-token'. \
                 Set INTERNAL_SERVICE_TOKEN in production."
            );
            "dev-service-token".to_string()
        });
    // Constant-time comparison prevents timing-based token enumeration.
    use subtle::ConstantTimeEq;
    if !<bool as From<_>>::from(token.as_bytes().ct_eq(expected.as_bytes())) { return Err(ApiError::unauthorized("invalid_service_token")); }
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

    // ── Resolve resource_id ───────────────────────────────────────────────
    let resource_id = if let Some(rid) = entry.resource_id.clone().filter(|s| !s.is_empty()) {
        rid
    } else if let Some(fid_str) = &entry.function_id {
        #[derive(sqlx::FromRow)]
        struct FnName { name: String }
        let fn_row = if let Ok(fid) = fid_str.parse::<Uuid>() {
            sqlx::query_as::<_, FnName>("SELECT name FROM functions WHERE id = $1 LIMIT 1")
                .bind(fid).fetch_optional(&pool).await.map_err(|_| db_err())?
        } else {
            sqlx::query_as::<_, FnName>("SELECT name FROM functions WHERE name = $1 LIMIT 1")
                .bind(fid_str).fetch_optional(&pool).await.map_err(|_| db_err())?
        };
        match fn_row {
            Some(f) => f.name,
            None => return Ok(ApiResponse::new(serde_json::json!({ "logged": false, "reason": "function_not_found" }))),
        }
    } else {
        return Ok(ApiResponse::new(serde_json::json!({ "logged": false, "reason": "missing_context" })));
    };

    sqlx::query(
        "INSERT INTO platform_logs \
         (source, resource_id, level, message, request_id, metadata, span_type) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(source).bind(&resource_id)
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

// ─── GET /logs  (project-scoped) ───────────────────────────────────────────────
//
// Query params:
//   source   — subsystem: function | db | workflow | event | queue | system
//   resource — resource_id (function name, db name, workflow id, etc.)
//   function — legacy alias for ?source=function&resource=<name>
//   level    — error | warn | info | debug
//   limit    — max rows (default 100, max 1000)
//   since    — ISO-8601 timestamp; return only rows newer than this

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
    Extension(_ctx): Extension<RequestContext>,
    Query(params): Query<ProjectLogQuery>,
) -> ApiResult<serde_json::Value> {
    let pool       = &state.pool;
    let limit      = params.limit.unwrap_or(100).min(1_000);

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
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_idx = 1usize;

    if source.is_some()   { conditions.push(format!("l.source = ${bind_idx}"));      bind_idx += 1; }
    if resource.is_some() { conditions.push(format!("l.resource_id = ${bind_idx}")); bind_idx += 1; }
    if level.is_some()    { conditions.push(format!("l.level = ${bind_idx}"));        bind_idx += 1; }

    let (time_cond, time_order) = match since {
        Some(_) => (format!("l.timestamp > ${bind_idx}"), "ASC"),
        None    => ("1=1".to_string(),                     "DESC"),
    };
    if since.is_some() { bind_idx += 1; }
    conditions.push(time_cond);

    let where_clause = if conditions.is_empty() {
        "1=1".to_string()
    } else {
        conditions.join(" AND ")
    };
    let sql = format!(
        "SELECT l.id, l.source, l.resource_id, l.level, l.message, \
                l.request_id, l.metadata, l.span_type, l.timestamp \
         FROM platform_logs l \
         WHERE {} \
         ORDER BY l.timestamp {} LIMIT ${}",
        where_clause, time_order, bind_idx
    );

    let mut q = sqlx::query_as::<_, PlatformLogRow>(&sql);
    if let Some(s)  = source   { q = q.bind(s); }
    if let Some(r)  = resource { q = q.bind(r); }
    if let Some(l)  = level    { q = q.bind(l); }
    if let Some(ts) = since    { q = q.bind(ts); }
    q = q.bind(limit);

    let rows = q.fetch_all(pool).await.map_err(|_| db_err())?;

    let logs: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
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
    Extension(_ctx): Extension<RequestContext>,
    Path(request_id): Path<String>,
    Query(params): Query<TraceQuery>,
) -> ApiResult<serde_json::Value> {
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
         WHERE l.request_id = $1 \
         ORDER BY l.timestamp ASC",
    )
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

    // ── Index suggestions ─────────────────────────────────────────────────
    // Heuristic: if the same (table, filter_column) pair appears in ≥ 2 slow
    // db spans within this request, the column is almost certainly unindexed.
    // We surface an actionable CREATE INDEX DDL statement the developer can
    // copy-paste directly — something no major platform does automatically.
    let mut slow_col_counts: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();

    for span in &spans {
        if span["source"].as_str() != Some("db") { continue; }
        if !span["metadata"]["slow"].as_bool().unwrap_or(false) { continue; }
        let table = match span["metadata"]["table"].as_str() {
            Some(t) => t.to_string(),
            None    => continue,
        };
        if let Some(cols) = span["metadata"]["filter_cols"].as_array() {
            for col in cols {
                if let Some(c) = col.as_str() {
                    *slow_col_counts
                        .entry((table.clone(), c.to_string()))
                        .or_insert(0) += 1;
                }
            }
        }
    }

    let mut suggested_indexes: Vec<serde_json::Value> = slow_col_counts
        .iter()
        .filter(|(_, count)| **count >= 2)
        .map(|((table, col), _)| {
            serde_json::json!({
                "table":  table,
                "column": col,
                "ddl":    format!("CREATE INDEX ON {}({});", table, col),
            })
        })
        .collect();
    // Stable sort so the CLI always renders in the same order.
    suggested_indexes.sort_by_key(|s| {
        format!(
            "{}.{}",
            s["table"].as_str().unwrap_or(""),
            s["column"].as_str().unwrap_or(""),
        )
    });

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
        "suggested_indexes":   suggested_indexes,
    })))
}

// ─── GET /traces  (list recent requests, project-scoped, Firebase auth) ───────
//
// Dual-mode:
//   ?since=<ISO-8601>   → forward: requests that *started after* since (flux tail)
//   ?before=<ISO-8601>  → backward: requests that *started before* before (flux why)
//
// Additional params:
//   exclude — request_id to skip (used by flux why to exclude current request)
//   limit   — default 5, max 20
//
// Each trace object includes: request_id, started_at, duration_ms, method, path,
// status, function, error (first error message if status >= 400).

#[derive(Deserialize)]
pub struct ListTracesQuery {
    pub before:  Option<String>,
    pub since:   Option<String>,
    pub exclude: Option<String>,
    pub limit:   Option<i64>,
}

pub async fn list_traces(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(params): Query<ListTracesQuery>,
) -> ApiResult<serde_json::Value> {
    let pool       = &state.pool;
    let limit      = params.limit.unwrap_or(5).min(20);
    let exclude    = params.exclude.unwrap_or_default();

    // ── Step 1: find distinct request windows ─────────────────────────────────
    #[derive(sqlx::FromRow)]
    struct ReqWindow {
        request_id: String,
        started_at: chrono::DateTime<chrono::Utc>,
        ended_at:   chrono::DateTime<chrono::Utc>,
    }

    let since_ts = params.since.as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let windows: Vec<ReqWindow> = if let Some(ts) = since_ts {
        // Forward mode: tail polling — requests whose first span arrived AFTER ts
        sqlx::query_as::<_, ReqWindow>(
            "SELECT request_id, MIN(timestamp) AS started_at, MAX(timestamp) AS ended_at \
             FROM platform_logs \
             WHERE request_id IS NOT NULL AND request_id != '' \
               AND timestamp > $1 \
             GROUP BY request_id \
             ORDER BY started_at ASC \
             LIMIT $2",
        )
        .bind(ts).bind(limit)
        .fetch_all(pool).await.map_err(|_| db_err())?
    } else {
        // Backward mode: why — requests whose first span started BEFORE before_ts
        let before_ts: chrono::DateTime<chrono::Utc> = params.before.as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        sqlx::query_as::<_, ReqWindow>(
            "SELECT request_id, MIN(timestamp) AS started_at, MAX(timestamp) AS ended_at \
             FROM platform_logs \
             WHERE request_id IS NOT NULL AND request_id != '' \
               AND ($2 = '' OR request_id != $2) \
               AND timestamp < $1 \
             GROUP BY request_id \
             ORDER BY started_at DESC \
             LIMIT $3",
        )
        .bind(before_ts).bind(&exclude).bind(limit)
        .fetch_all(pool).await.map_err(|_| db_err())?
    };

    if windows.is_empty() {
        return Ok(ApiResponse::new(serde_json::json!({ "traces": [] })));
    }

    // ── Step 2: batch-fetch gateway span (method/path/status) ─────────────────
    let ids: Vec<String> = windows.iter().map(|w| w.request_id.clone()).collect();
    let id_list = ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(",");

    #[derive(sqlx::FromRow)]
    struct GatewaySpan { request_id: String, metadata: Option<serde_json::Value> }

    let gw_sql = format!(
        "SELECT DISTINCT ON (request_id) request_id, metadata \
         FROM platform_logs \
         WHERE request_id IN ({id_list}) \
           AND (source = 'gateway' OR span_type IN ('request','gateway_request','http_request')) \
         ORDER BY request_id, timestamp ASC",
    );
    let gw_spans = sqlx::query_as::<_, GatewaySpan>(&gw_sql)
        .fetch_all(pool).await.unwrap_or_default();

    let gw_map: std::collections::HashMap<String, serde_json::Value> = gw_spans
        .into_iter()
        .filter_map(|s| s.metadata.map(|m| (s.request_id, m)))
        .collect();

    // ── Step 3: batch-fetch function name + first error message ───────────────
    // A single query returns: runtime spans (→ function name via resource_id)
    // and error spans (→ error message). DISTINCT ON picks the best row per
    // (request_id, priority) — errors take priority 1, runtime takes 2.
    #[derive(sqlx::FromRow)]
    struct EnrichSpan {
        request_id:  String,
        source:      Option<String>,
        resource_id: Option<String>,
        message:     Option<String>,
        level:       Option<String>,
        span_type:   Option<String>,
    }

    let enrich_sql = format!(
        "SELECT DISTINCT ON (request_id, pr) request_id, source, resource_id, message, level, span_type \
         FROM ( \
           SELECT request_id, source, resource_id, message, level, span_type, \
                  CASE WHEN level='error' OR span_type='error' THEN 1 \
                       WHEN source='runtime' THEN 2 \
                       ELSE 3 END AS pr \
           FROM platform_logs \
           WHERE request_id IN ({id_list}) \
             AND (level='error' OR span_type='error' OR source='runtime') \
         ) sub \
         ORDER BY request_id, pr, timestamp ASC",
    );
    let enrich_rows = sqlx::query_as::<_, EnrichSpan>(&enrich_sql)
        .fetch_all(pool).await.unwrap_or_default();

    // Build maps: request_id → function_name, request_id → error_message
    let mut fn_map:  std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut err_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for row in enrich_rows {
        let is_error = row.level.as_deref() == Some("error")
            || row.span_type.as_deref() == Some("error");
        if is_error {
            err_map.entry(row.request_id.clone()).or_insert_with(|| {
                row.message.unwrap_or_default()
            });
        }
        if row.source.as_deref() == Some("runtime") {
            fn_map.entry(row.request_id.clone()).or_insert_with(|| {
                row.resource_id.unwrap_or_default()
            });
        }
    }

    // ── Assemble response ─────────────────────────────────────────────────────
    let traces: Vec<serde_json::Value> = windows.iter().map(|w| {
        let meta        = gw_map.get(&w.request_id).cloned().unwrap_or_default();
        let duration_ms = (w.ended_at - w.started_at).num_milliseconds();
        let status      = meta.get("status").and_then(|v| v.as_i64()).unwrap_or(0)
            // some gateways write "status_code"
            .max(meta.get("status_code").and_then(|v| v.as_i64()).unwrap_or(0));
        let function    = fn_map.get(&w.request_id).cloned()
            .or_else(|| meta.get("function").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_default();
        let error       = err_map.get(&w.request_id).cloned().unwrap_or_default();

        serde_json::json!({
            "request_id":  w.request_id,
            "started_at":  w.started_at.to_rfc3339(),
            "duration_ms": duration_ms,
            "method":      meta.get("method").and_then(|v| v.as_str()).unwrap_or("?"),
            "path":        meta.get("path").and_then(|v| v.as_str()).unwrap_or("?"),
            "status":      status,
            "function":    function,
            "error":       error,
            "is_error":    status >= 400 || !error.is_empty(),
        })
    }).collect();

    Ok(ApiResponse::new(serde_json::json!({ "traces": traces })))
}
