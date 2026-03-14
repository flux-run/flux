//! SSE streaming endpoints — live feed of executions, events, and mutations.
//!
//! All three endpoints use cursor-based DB polling (500 ms interval) so they
//! survive process restarts and work through standard HTTP load balancers.
//!
//! Resumption: send `Last-Event-ID` header (or `?since=<rfc3339>`) to pick up
//! from the last received event.  `EventSource` does this automatically on
//! reconnect.

use axum::{
    Extension,
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, Utc};
use futures_util::stream::{self, Stream, StreamExt as _};
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{app::AppState, types::context::RequestContext};

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StreamParams {
    /// ISO 8601 timestamp; stream events created after this point.
    pub since: Option<String>,
}

fn cursor_from(headers: &HeaderMap, since: Option<String>) -> DateTime<Utc> {
    let s = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or(since);
    s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        // Default: start from now so we only tail new activity.
        .unwrap_or_else(Utc::now)
}

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct EventStreamRow {
    id: Uuid,
    event_type: String,
    table_name: String,
    operation: String,
    record_id: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ExecutionRow {
    request_id: String,
    method: String,
    path: String,
    response_status: Option<i32>,
    duration_ms: Option<i32>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct MutationRow {
    request_id: Option<String>,
    table_name: String,
    operation: String,
    record_pk: Option<String>,
    mutation_seq: Option<i64>,
    mutation_ts: Option<DateTime<Utc>>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /stream/events`
///
/// Live SSE stream of all events: system events (table.inserted, etc.) and
/// custom events published via `ctx.events.emit()`.
pub async fn stream_events(
    headers: HeaderMap,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(params): Query<StreamParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cursor = cursor_from(&headers, params.since);
    let pool = state.pool.clone();

    let s = stream::unfold(cursor, move |cursor| {
        let pool = pool.clone();
        async move {
            sleep(Duration::from_millis(500)).await;

            let rows: Vec<EventStreamRow> = sqlx::query_as(
                r#"
                SELECT id, event_type, table_name, operation, record_id, created_at
                FROM   fluxbase_internal.events
                WHERE  created_at > $1
                ORDER  BY created_at ASC
                LIMIT  50
                "#,
            )
            .bind(cursor)
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

            let new_cursor = rows.last().map(|r| r.created_at).unwrap_or(cursor);

            let events: Vec<Result<Event, Infallible>> = rows
                .into_iter()
                .map(|r| {
                    let data = serde_json::json!({
                        "id":         r.id,
                        "event_type": r.event_type,
                        "table":      r.table_name,
                        "operation":  r.operation,
                        "record_id":  r.record_id,
                        "ts":         r.created_at,
                    });
                    Ok(Event::default()
                        .id(new_cursor.to_rfc3339())
                        .event("event")
                        .data(data.to_string()))
                })
                .collect();

            Some((stream::iter(events), new_cursor))
        }
    })
    .flatten();

    Sse::new(s).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// `GET /stream/executions`
///
/// Live SSE stream of every data-engine request (from `trace_requests`).
/// Each event includes method, path, status, and duration so the dashboard
/// can show a live request log.
pub async fn stream_executions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(params): Query<StreamParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cursor = cursor_from(&headers, params.since);
    let pool = state.pool.clone();

    let s = stream::unfold(cursor, move |cursor| {
        let pool = pool.clone();
        async move {
            sleep(Duration::from_millis(500)).await;

            let rows: Vec<ExecutionRow> = sqlx::query_as(
                r#"
                SELECT request_id, method, path, response_status, duration_ms, created_at
                FROM   fluxbase_internal.trace_requests
                WHERE  created_at > $1
                ORDER  BY created_at ASC
                LIMIT  50
                "#,
            )
            .bind(cursor)
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

            let new_cursor = rows.last().map(|r| r.created_at).unwrap_or(cursor);

            let events: Vec<Result<Event, Infallible>> = rows
                .into_iter()
                .map(|r| {
                    let ok = r.response_status.map(|s| s < 400).unwrap_or(true);
                    let data = serde_json::json!({
                        "request_id":  r.request_id,
                        "method":      r.method,
                        "path":        r.path,
                        "status":      r.response_status,
                        "duration_ms": r.duration_ms,
                        "ok":          ok,
                        "ts":          r.created_at,
                    });
                    Ok(Event::default()
                        .id(new_cursor.to_rfc3339())
                        .event("execution")
                        .data(data.to_string()))
                })
                .collect();

            Some((stream::iter(events), new_cursor))
        }
    })
    .flatten();

    Sse::new(s).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// `GET /stream/mutations`
///
/// Live SSE stream of individual row mutations from `state_mutations`.
/// Use this for building real-time UI that reacts to specific table changes
/// (e.g. "show a badge when a new order is inserted").
pub async fn stream_mutations(
    headers: HeaderMap,
    State(state): State<AppState>,
    Extension(_ctx): Extension<RequestContext>,
    Query(params): Query<StreamParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cursor = cursor_from(&headers, params.since);
    let pool = state.pool.clone();

    let s = stream::unfold(cursor, move |cursor| {
        let pool = pool.clone();
        async move {
            sleep(Duration::from_millis(500)).await;

            let rows: Vec<MutationRow> = sqlx::query_as(
                r#"
                SELECT request_id, table_name, operation, record_pk, mutation_seq, mutation_ts
                FROM   fluxbase_internal.state_mutations
                WHERE  mutation_ts > $1
                ORDER  BY mutation_ts ASC
                LIMIT  100
                "#,
            )
            .bind(cursor)
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

            let new_cursor = rows
                .last()
                .and_then(|r| r.mutation_ts)
                .unwrap_or(cursor);

            let events: Vec<Result<Event, Infallible>> = rows
                .into_iter()
                .map(|r| {
                    let ts = r.mutation_ts.unwrap_or(cursor);
                    let data = serde_json::json!({
                        "request_id":  r.request_id,
                        "table":       r.table_name,
                        "operation":   r.operation,
                        "record_id":   r.record_pk,
                        "seq":         r.mutation_seq,
                        "ts":          ts,
                    });
                    Ok(Event::default()
                        .id(new_cursor.to_rfc3339())
                        .event("mutation")
                        .data(data.to_string()))
                })
                .collect();

            Some((stream::iter(events), new_cursor))
        }
    })
    .flatten();

    Sse::new(s).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
