/// Cross-table mutation lookup by request_id — used by `flux why`.
///
///   GET /db/mutations?request_id=<id>&limit=<n>
///
/// Returns all state_mutations rows for that request across every table,
/// ordered by version (oldest first).  The caller (CLI) uses this to show
/// what data changed during a specific traced request.
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
pub struct MutationsParams {
    /// The request_id to look up (required).
    pub request_id: String,
    /// Max rows (default 100, max 500).
    pub limit: Option<u32>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct MutationRow {
    pub table_name:   String,
    pub record_pk:    serde_json::Value,
    pub operation:    String,
    pub before_state: Option<serde_json::Value>,
    pub after_state:  Option<serde_json::Value>,
    pub actor_id:     Option<String>,
    pub version:      i64,
    pub created_at:   DateTime<Utc>,
}

/// GET /db/mutations?request_id=<id>&limit=<n>
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<MutationsParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers)
        .map_err(EngineError::MissingField)?;

    let limit = params.limit.unwrap_or(100).min(500) as i64;

    let rows = sqlx::query_as::<_, MutationRow>(
        r#"
        SELECT
            table_name,
            record_pk,
            operation,
            before_state,
            after_state,
            actor_id,
            version,
            created_at
        FROM fluxbase_internal.state_mutations
        WHERE tenant_id   = $1
          AND project_id  = $2
          AND request_id  = $3
        ORDER BY created_at, version
        LIMIT $4
        "#,
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&params.request_id)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(json!({
        "request_id": params.request_id,
        "count": rows.len(),
        "mutations": rows,
    })))
}
