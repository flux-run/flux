/// Realtime SSE event streaming.
///
/// GET  /events/stream         — subscribe to table-change events for this project.
/// POST /internal/events/emit  — publish a change event (called by the runtime / data engine).
///
/// ### Event format (SSE data field, JSON)
/// ```json
/// { "table": "users", "operation": "insert", "row": { ... } }
/// ```
///
/// ### Query parameters for GET /events/stream
/// | Name      | Type   | Description                                  |
/// |-----------|--------|----------------------------------------------|
/// | table     | string | Only receive events for this table (optional)|
/// | operation | string | `insert`, `update`, or `delete` (optional)   |
///
/// ### Internal publish body (POST /internal/events/emit)
/// ```json
/// { "project_id": "...", "table": "users", "operation": "insert", "row": { ... } }
/// ```

use std::convert::Infallible;

use axum::{
    extract::{Extension, Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::{
    types::{
        context::RequestContext,
        response::{ApiError, ApiResponse},
    },
    AppState,
};

// ─── Shared event shape ────────────────────────────────────────────────────────

/// The full event envelope written to the broadcast channel.
/// Serialised to JSON and used as the SSE `data:` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableChangeEvent {
    /// Project that owns this event — used to fan-out to the right SSE clients.
    pub project_id: String,
    /// Database table name.
    pub table: String,
    /// `insert`, `update`, or `delete`.
    pub operation: String,
    /// The affected row (insert/update = new values; delete = deleted row).
    #[serde(default)]
    pub row: Value,
}

/// The outward-facing payload sent as the SSE `data:` field (no project_id).
#[derive(Debug, Serialize)]
struct OutboundEvent<'a> {
    table:     &'a str,
    operation: &'a str,
    row:       &'a Value,
}

// ─── GET /events/stream ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    /// If provided, only events for this table are forwarded to the client.
    pub table: Option<String>,
    /// If provided, only events with this operation are forwarded (`insert`,
    /// `update`, `delete`).
    pub operation: Option<String>,
}

/// Subscribe to realtime table-change events for the current project.
///
/// Returns an SSE stream.  Closes when the client disconnects or the server
/// shuts down.  Lagging consumers (slow readers) silently skip messages.
pub async fn stream(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(q): Query<StreamQuery>,
) -> Response {
    // Project scope middleware guarantees project_id is set, but guard anyway.
    let project_id = match ctx.project_id {
        Some(id) => id.to_string(),
        None => {
            return ApiError::bad_request("project_id_required").into_response();
        }
    };
    let filter_table = q.table;
    let filter_op    = q.operation;

    let rx = state.event_tx.subscribe();
    let sse_stream = BroadcastStream::new(rx).filter_map(move |msg| {
        let project_id    = project_id.clone();
        let filter_table  = filter_table.clone();
        let filter_op     = filter_op.clone();

        let result: Option<Result<Event, Infallible>> = match msg {
            Err(_lagged) => None, // subscriber too slow — skip
            Ok(raw) => {
                let ev: TableChangeEvent = match serde_json::from_str(&raw) {
                    Ok(e)  => e,
                    Err(_) => return None,
                };

                // Project filter — only forward events for this project.
                if ev.project_id != project_id {
                    return None;
                }
                // Table filter.
                if let Some(ref t) = filter_table {
                    if &ev.table != t {
                        return None;
                    }
                }
                // Operation filter.
                if let Some(ref op) = filter_op {
                    if &ev.operation != op {
                        return None;
                    }
                }

                let outbound = OutboundEvent {
                    table:     &ev.table,
                    operation: &ev.operation,
                    row:       &ev.row,
                };
                let data = serde_json::to_string(&outbound).unwrap_or_default();
                Some(Ok(Event::default().data(data)))
            }
        };
        result
    });

    Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ─── POST /internal/events/emit ───────────────────────────────────────────────

/// Publish a table-change event.  Called by the runtime / data engine after
/// a successful write so connected SSE clients receive it in real time.
///
/// Any service with access to the `/internal/` prefix can call this.
pub async fn emit(
    State(state): State<AppState>,
    Json(event): Json<TableChangeEvent>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    let payload = serde_json::to_string(&event)
        .map_err(|e| ApiError::internal(&format!("event_ser: {}", e)))?;

    // `send` returns Err only when there are *no* active receivers — that is
    // perfectly normal (no clients connected) and not an error condition.
    let _ = state.event_tx.send(payload);

    Ok(ApiResponse::new(serde_json::json!({ "ok": true })))
}
