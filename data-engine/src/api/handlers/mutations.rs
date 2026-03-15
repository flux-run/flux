/// Cross-table mutation lookup by request_id — used by `flux why` and `flux trace diff`.
///
///   GET /db/mutations?request_id=<id>&limit=<n>&table_name=<t>&after_seq=<cursor>
///
/// Returns state_mutations rows for that request, ordered by mutation_seq (strict
/// total write order, deterministic).  Supports keyset pagination via `after_seq`
/// so callers can page through 100k+ mutation logs in O(page_size) memory.
///
/// Table filter (`table_name`) limits results to one table only, e.g. for
/// `flux trace diff --table users` which is ~10–100× smaller than the full log.
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
    /// Max rows per page (default 500, max 1000).
    pub limit: Option<u32>,
    /// Optional table name filter — only return mutations for this table.
    /// Backed by idx_state_mutations_request_table.
    pub table_name: Option<String>,
    /// Keyset pagination cursor: return only rows with mutation_seq > this value.
    /// Set to the `next_after_seq` from the previous response to get the next page.
    /// Omit (or set to 0) to start from the beginning.
    pub after_seq: Option<i64>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct MutationRow {
    pub mutation_seq:  i64,
    pub table_name:    String,
    pub record_pk:     serde_json::Value,
    pub operation:     String,
    pub before_state:  Option<serde_json::Value>,
    pub after_state:   Option<serde_json::Value>,
    pub changed_fields: Option<Vec<String>>,
    pub actor_id:      Option<String>,
    pub span_id:       Option<String>,
    pub schema_name:   Option<String>,
    pub version:       i64,
    pub created_at:    DateTime<Utc>,
}

/// GET /db/mutations?request_id=<id>[&limit=N][&table_name=T][&after_seq=C]
pub async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<MutationsParams>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth    = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let limit     = params.limit.unwrap_or(500).min(1000) as i64;
    let after_seq = params.after_seq.unwrap_or(0);

    // Two query shapes: with and without table_name filter.
    // Both use (request_id, mutation_seq) index for O(log N) keyset pagination.
    // The table_name variant additionally uses idx_state_mutations_request_table
    // to reduce rows scanned on large logs (e.g. 100k mutations, one table has 500).
    let rows: Vec<MutationRow> = if let Some(ref tbl) = params.table_name {
        sqlx::query_as::<_, MutationRow>(
            r#"
            SELECT
                mutation_seq,
                table_name,
                record_pk,
                operation,
                before_state,
                after_state,
                changed_fields,
                actor_id,
                span_id,
                schema_name,
                version,
                created_at
            FROM flux_internal.state_mutations
            WHERE request_id   = $1
              AND table_name   = $2
              AND mutation_seq > $3
            ORDER BY mutation_seq
            LIMIT $4
            "#,
        )
        .bind(&params.request_id)
        .bind(tbl)
        .bind(after_seq)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, MutationRow>(
            r#"
            SELECT
                mutation_seq,
                table_name,
                record_pk,
                operation,
                before_state,
                after_state,
                changed_fields,
                actor_id,
                span_id,
                schema_name,
                version,
                created_at
            FROM flux_internal.state_mutations
            WHERE request_id   = $1
              AND mutation_seq > $2
            ORDER BY mutation_seq
            LIMIT $3
            "#,
        )
        .bind(&params.request_id)
        .bind(after_seq)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };

    // Expose the cursor value for the next page.  When `next_after_seq` is null
    // the caller has reached the end of the log.
    let next_after_seq: Option<i64> = if rows.len() as i64 == limit {
        rows.last().map(|r| r.mutation_seq)
    } else {
        None
    };

    Ok(Json(json!({
        "request_id":    params.request_id,
        "count":         rows.len(),
        "next_after_seq": next_after_seq,
        "mutations":     rows,
    })))
}
