//! `/records/*` — export, count, and prune execution records from `platform_logs`.
//!
//! All handlers are project-scoped via `RequestContext`. Filters:
//!   `before`      — age spec like "30d", "7d", "24h"; matches rows OLDER than this
//!   `after`       — age spec like "30d", "7d", "24h"; matches rows NEWER than this
//!   `function`    — filter to a specific function name (resource_id)
//!   `errors_only` — only include rows with level = 'error'
//!
//! Export additionally accepts `format=jsonl` (default) or `format=csv`.

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::Response,
    body::Body,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{PgPool, QueryBuilder};
use uuid::Uuid;

use crate::error::{ApiError, ApiResponse, ApiResult};
use crate::types::context::RequestContext;

// ── Age spec parser ───────────────────────────────────────────────────────────

/// Parse "30d" → `Utc::now() - 30 days`, "24h" → `Utc::now() - 24 hours`.
fn parse_age(spec: &str) -> Option<DateTime<Utc>> {
    let s = spec.trim();
    if let Some(n_str) = s.strip_suffix('d') {
        let n: i64 = n_str.parse().ok()?;
        Some(Utc::now() - chrono::Duration::days(n))
    } else if let Some(n_str) = s.strip_suffix('h') {
        let n: i64 = n_str.parse().ok()?;
        Some(Utc::now() - chrono::Duration::hours(n))
    } else {
        None
    }
}

// ── Shared query params ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordsQuery {
    pub before:      Option<String>,
    pub after:       Option<String>,
    pub function:    Option<String>,
    pub errors_only: Option<bool>,
    /// Export only: "jsonl" (default) | "csv"
    pub format:      Option<String>,
}

#[derive(Deserialize)]
pub struct PruneQuery {
    pub before: Option<String>,
}

// ── Row type for export ───────────────────────────────────────────────────────

#[derive(sqlx::FromRow, serde::Serialize)]
struct RecordRow {
    pub id:          Uuid,
    pub level:       String,
    pub message:     String,
    pub timestamp:   DateTime<Utc>,
    pub source:      String,
    pub resource_id: String,
    pub request_id:  Option<String>,
    pub metadata:    Option<serde_json::Value>,
}

// ── GET /records/count ────────────────────────────────────────────────────────

pub async fn records_count(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    Query(params): Query<RecordsQuery>,
) -> ApiResult<serde_json::Value> {
    let before_ts   = params.before.as_deref().and_then(parse_age);
    let after_ts    = params.after.as_deref().and_then(parse_age);
    let errors_only = params.errors_only.unwrap_or(false);

    let mut qb = QueryBuilder::<sqlx::Postgres>::new(
        "SELECT COUNT(*) FROM platform_logs WHERE project_id = ",
    );
    qb.push_bind(ctx.project_id);

    if errors_only {
        qb.push(" AND level = 'error'");
    }
    if let Some(fname) = &params.function {
        qb.push(" AND source = 'function' AND resource_id = ");
        qb.push_bind(fname.as_str());
    }
    if let Some(ts) = before_ts {
        qb.push(" AND timestamp < ");
        qb.push_bind(ts);
    }
    if let Some(ts) = after_ts {
        qb.push(" AND timestamp > ");
        qb.push_bind(ts);
    }

    let count: i64 = qb
        .build_query_scalar()
        .fetch_one(&pool)
        .await
        .map_err(ApiError::from)?;

    Ok(ApiResponse::new(serde_json::json!({ "count": count })))
}

// ── GET /records/export ───────────────────────────────────────────────────────

pub async fn records_export(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    Query(params): Query<RecordsQuery>,
) -> Result<Response, ApiError> {
    let before_ts   = params.before.as_deref().and_then(parse_age);
    let after_ts    = params.after.as_deref().and_then(parse_age);
    let errors_only = params.errors_only.unwrap_or(false);
    let format      = params.format.as_deref().unwrap_or("jsonl");

    let mut qb = QueryBuilder::<sqlx::Postgres>::new(
        "SELECT id, level, message, timestamp, source, resource_id, request_id, metadata \
         FROM platform_logs WHERE project_id = ",
    );
    qb.push_bind(ctx.project_id);

    if errors_only {
        qb.push(" AND level = 'error'");
    }
    if let Some(fname) = &params.function {
        qb.push(" AND source = 'function' AND resource_id = ");
        qb.push_bind(fname.as_str());
    }
    if let Some(ts) = before_ts {
        qb.push(" AND timestamp < ");
        qb.push_bind(ts);
    }
    if let Some(ts) = after_ts {
        qb.push(" AND timestamp > ");
        qb.push_bind(ts);
    }
    qb.push(" ORDER BY timestamp ASC");

    let rows: Vec<RecordRow> = qb
        .build_query_as()
        .fetch_all(&pool)
        .await
        .map_err(ApiError::from)?;

    let (content_type, body) = match format {
        "csv" => {
            let mut out = String::from(
                "id,level,message,timestamp,source,resource_id,request_id\n",
            );
            for r in &rows {
                // Escape commas in message by replacing with semicolon
                let msg = r.message.replace(',', ";").replace('\n', " ");
                out.push_str(&format!(
                    "{},{},{},{},{},{},{}\n",
                    r.id,
                    r.level,
                    msg,
                    r.timestamp.to_rfc3339(),
                    r.source,
                    r.resource_id,
                    r.request_id.as_deref().unwrap_or(""),
                ));
            }
            ("text/csv", out)
        }
        _ => {
            let body = rows
                .iter()
                .filter_map(|r| serde_json::to_string(r).ok())
                .collect::<Vec<_>>()
                .join("\n");
            ("application/x-ndjson", body)
        }
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .body(Body::from(body))
        .unwrap();
    Ok(response)
}

// ── DELETE /records/prune ─────────────────────────────────────────────────────

pub async fn records_prune(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    Query(params): Query<PruneQuery>,
) -> ApiResult<serde_json::Value> {
    let before_ts = params.before.as_deref().and_then(parse_age);

    if before_ts.is_none() {
        return Err(ApiError::bad_request(
            "Missing required parameter: provide ?before=<age> (e.g. ?before=30d)",
        ));
    }

    let mut qb = QueryBuilder::<sqlx::Postgres>::new(
        "DELETE FROM platform_logs WHERE project_id = ",
    );
    qb.push_bind(ctx.project_id);

    if let Some(ts) = before_ts {
        qb.push(" AND timestamp < ");
        qb.push_bind(ts);
    }

    let result = qb.build().execute(&pool).await.map_err(ApiError::from)?;
    let deleted = result.rows_affected();

    Ok(ApiResponse::new(serde_json::json!({ "deleted": deleted })))
}
