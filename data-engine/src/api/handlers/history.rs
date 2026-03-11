/// Audit-trail read endpoints — Gap 1 read surface.
///
/// These three handlers expose the data collected by `db_executor::execute()`
/// into `fluxbase_internal.state_mutations`.  They are intentionally
/// read-only, tenant-scoped, and require no schema prefix because
/// `fluxbase_internal` is a shared service schema.
///
///   GET /db/history/:database/:table   — full version history for one row
///   GET /db/blame/:database/:table     — last writer per row in a table
///   GET /db/replay/:database           — all mutations in a time window
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    state::AppState,
};

// ─── shared row types ────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
pub struct HistoryRow {
    pub version:      i64,
    pub operation:    String,
    pub before_state: Option<serde_json::Value>,
    pub after_state:  Option<serde_json::Value>,
    pub actor_id:     Option<String>,
    pub request_id:   Option<String>,
    pub created_at:   DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct BlameRow {
    pub record_pk:  serde_json::Value,
    pub actor_id:   Option<String>,
    pub request_id: Option<String>,
    pub version:    i64,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct ReplayRow {
    pub table_name:   String,
    pub record_pk:    serde_json::Value,
    pub operation:    String,
    pub before_state: Option<serde_json::Value>,
    pub after_state:  Option<serde_json::Value>,
    pub actor_id:     Option<String>,
    pub request_id:   Option<String>,
    pub version:      i64,
    pub created_at:   DateTime<Utc>,
}

// ─── query-string param structs ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct HistoryParams {
    /// Simple scalar pk: ?id=42  →  record_pk = {"id": 42}
    pub id:    Option<String>,
    /// Composite / custom pk as JSON string: ?pk={"user_id":1,"tenant":"a"}
    pub pk:    Option<String>,
    /// Max rows to return (default 50, max 500)
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct BlameParams {
    /// Max rows (default 100, max 1000)
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct ReplayParams {
    /// RFC-3339 start timestamp (inclusive), e.g. 2026-03-09T15:00:00Z
    pub from:  String,
    /// RFC-3339 end timestamp (inclusive), e.g. 2026-03-09T15:05:00Z
    pub to:    String,
    /// Max rows (default 500, max 2000)
    pub limit: Option<u32>,
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Parse record_pk from query params.
/// Accepts either ?id=<scalar> (the 99% case) or ?pk=<json> for composite keys.
fn parse_record_pk(params: &HistoryParams) -> Result<serde_json::Value, EngineError> {
    if let Some(id_str) = &params.id {
        // Try integer, then float, then fall back to string.
        if let Ok(n) = id_str.parse::<i64>() {
            return Ok(json!({ "id": n }));
        }
        if let Ok(f) = id_str.parse::<f64>() {
            return Ok(json!({ "id": f }));
        }
        return Ok(json!({ "id": id_str }));
    }
    if let Some(pk_str) = &params.pk {
        return serde_json::from_str(pk_str)
            .map_err(|_| EngineError::MissingField(
                "?pk value is not valid JSON — use e.g. ?pk={\"id\":42}".into()
            ));
    }
    Err(EngineError::MissingField(
        "provide ?id=<value> for simple pk or ?pk=<json> for composite keys".into()
    ))
}

// ─── GET /db/history/:database/:table ───────────────────────────────────────

/// Return the complete version history for a single row, newest-first.
///
/// Query params:
///   id=<scalar>   — pk value when the pk column is named "id"
///   pk=<json>     — arbitrary JSON for composite or non-"id" pks
///   limit=<n>     — max rows (default 50, cap 500)
///
/// Example:
///   GET /db/history/main/users?id=42
///   GET /db/history/main/orders?pk={"order_id":"ORD-99"}
pub async fn history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_database, table)): Path<(String, String)>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let record_pk = parse_record_pk(&params)?;
    let limit = params.limit.unwrap_or(50).min(500) as i64;

    let rows = sqlx::query_as::<_, HistoryRow>(
        r#"
        SELECT version, operation, before_state, after_state,
               actor_id, request_id, created_at
        FROM   fluxbase_internal.state_mutations
        WHERE  tenant_id  = $1
          AND  project_id = $2
          AND  table_name = $3
          AND  record_pk  = $4
        ORDER  BY version DESC
        LIMIT  $5
        "#,
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&table)
    .bind(&record_pk)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(Json(json!({
        "table":     table,
        "record_pk": record_pk,
        "history":   rows,
        "count":     rows.len(),
    })))
}

// ─── GET /db/blame/:database/:table ─────────────────────────────────────────

/// Return the last writer for every distinct row that has been mutated.
/// Uses DISTINCT ON so each record_pk appears only once, with the highest
/// version (most recent mutation) selected.
///
/// Query params:
///   limit=<n>  — max distinct rows (default 100, cap 1000)
///
/// Example:
///   GET /db/blame/main/users
pub async fn blame(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_database, table)): Path<(String, String)>,
    Query(params): Query<BlameParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let limit = params.limit.unwrap_or(100).min(1000) as i64;

    // DISTINCT ON (record_pk) with ORDER BY record_pk, version DESC gives the
    // highest-version row per record_pk in a single index scan over
    // idx_state_mutations_pk_version.
    let rows = sqlx::query_as::<_, BlameRow>(
        r#"
        SELECT DISTINCT ON (record_pk)
               record_pk, actor_id, request_id, version, created_at
        FROM   fluxbase_internal.state_mutations
        WHERE  tenant_id  = $1
          AND  project_id = $2
          AND  table_name = $3
        ORDER  BY record_pk, version DESC
        LIMIT  $4
        "#,
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&table)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(Json(json!({
        "table": table,
        "blame": rows,
        "count": rows.len(),
    })))
}

// ─── GET /db/replay/:database ────────────────────────────────────────────────

/// Return all mutations within a time window, ordered by created_at ASC.
/// This is the foundation for `flux incident replay` and `flux db replay`.
///
/// Query params:
///   from=<RFC-3339>   — window start (inclusive), e.g. 2026-03-09T15:00:00Z
///   to=<RFC-3339>     — window end   (inclusive)
///   limit=<n>         — max rows (default 500, cap 2000)
///
/// Example:
///   GET /db/replay/main?from=2026-03-09T15:00:00Z&to=2026-03-09T15:05:00Z
pub async fn replay(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(_database): Path<String>,
    Query(params): Query<ReplayParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let limit = params.limit.unwrap_or(500).min(2000) as i64;

    let from: DateTime<Utc> = params.from.parse::<DateTime<Utc>>().map_err(|_| {
        EngineError::MissingField(
            "?from must be an RFC-3339 timestamp, e.g. 2026-03-09T15:00:00Z".into(),
        )
    })?;
    let to: DateTime<Utc> = params.to.parse::<DateTime<Utc>>().map_err(|_| {
        EngineError::MissingField(
            "?to must be an RFC-3339 timestamp, e.g. 2026-03-09T15:05:00Z".into(),
        )
    })?;

    if from >= to {
        return Err(EngineError::MissingField(
            "?from must be earlier than ?to".into(),
        ));
    }

    let rows = sqlx::query_as::<_, ReplayRow>(
        r#"
        SELECT table_name, record_pk, operation,
               before_state, after_state,
               actor_id, request_id, version, created_at
        FROM   fluxbase_internal.state_mutations
        WHERE  tenant_id  = $1
          AND  project_id = $2
          AND  created_at BETWEEN $3 AND $4
        ORDER  BY created_at ASC
        LIMIT  $5
        "#,
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(Json(json!({
        "from":    from,
        "to":      to,
        "replay":  rows,
        "count":   rows.len(),
    })))
}
