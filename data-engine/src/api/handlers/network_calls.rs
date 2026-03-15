/// Outbound network call log — recorded by the runtime for every ctx.fetch() call.
///
///   GET /db/network-calls?request_id=<id>[&limit=N][&after_id=C]
///
/// Returns network_calls rows for that request, ordered by call_seq.
/// Supports keyset pagination via `after_id` for large call logs.
///
/// Powers: `flux trace <id>` (shows all external calls in waterfall),
///         `flux incident replay <id>` (mock mode replays recorded responses),
///         resume-from-checkpoint (know which external calls already succeeded).
use axum::{
    extract::{Query, State},
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

#[derive(Deserialize)]
pub struct NetworkCallsParams {
    /// The request_id to look up (required).
    pub request_id: String,
    /// Max rows per page (default 200, max 1000).
    pub limit: Option<u32>,
    /// Keyset pagination cursor: return only rows with id > this value.
    pub after_id: Option<i64>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct NetworkCallRow {
    pub id:               i64,
    pub call_seq:         i32,
    pub method:           String,
    pub url:              String,
    pub host:             String,
    pub status:           Option<i32>,
    pub request_body:     Option<String>,
    pub response_body:    Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub duration_ms:      i32,
    pub error:            Option<String>,
    pub span_id:          Option<String>,
    pub created_at:       DateTime<Utc>,
}

/// GET /db/network-calls?request_id=<id>[&limit=N][&after_id=C]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<NetworkCallsParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth    = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let limit    = params.limit.unwrap_or(200).min(1000) as i64;
    let after_id = params.after_id.unwrap_or(0);

    let rows: Vec<NetworkCallRow> = sqlx::query_as::<_, NetworkCallRow>(
        r#"
        SELECT
            id,
            call_seq,
            method,
            url,
            host,
            status,
            request_body,
            response_body,
            response_headers,
            duration_ms,
            error,
            span_id,
            created_at
        FROM flux_internal.network_calls
        WHERE request_id = $1
          AND id > $2
        ORDER BY call_seq, id
        LIMIT $3
        "#,
    )
    .bind(&params.request_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    let next_after_id: Option<i64> = if rows.len() as i64 == limit {
        rows.last().map(|r| r.id)
    } else {
        None
    };

    let count = rows.len();
    Ok(Json(json!({
        "request_id":    params.request_id,
        "calls":         rows,
        "count":         count,
        "next_after_id": next_after_id,
    })))
}
