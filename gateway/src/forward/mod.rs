//! Runtime forwarding.
//!
//! Dispatches function-invocation requests to the Runtime service via the
//! `RuntimeDispatch` trait — either HTTP (multi-process) or in-process
//! (server crate).  Auth-context is threaded through as structured fields.

pub mod http_impl;
pub use http_impl::HttpRuntimeDispatch;

use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use job_contract::dispatch::ExecuteRequest;
use crate::auth::AuthContext;
use crate::snapshot::RouteRecord;
use crate::state::SharedState;

/// Forward a function-invocation request to the runtime via the dispatch trait.
///
/// Returns the runtime's response verbatim, with `x-request-id` echoed back.
pub async fn to_runtime(
    state:       &SharedState,
    route:       &RouteRecord,
    payload:     Value,
    request_id:  &str,
    parent_span: Option<&str>,
    auth_ctx:    &AuthContext,
) -> Response {
    // ── Build dispatch request ────────────────────────────────────────────
    let (user_id, jwt_claims) = match auth_ctx {
        AuthContext::Jwt { user_id, claims } => (
            user_id.clone().map(|s| s.to_string()),
            claims.clone(),
        ),
        _ => (None, None),
    };

    let req = ExecuteRequest {
        function_id:    route.function_id.to_string(),
        project_id:     Some(route.project_id),
        payload,
        execution_seed: None,
        request_id:     Some(request_id.to_string()),
        parent_span_id: parent_span.map(|s| s.to_string()),
        runtime_hint:   Some(route.runtime.clone()),
        user_id,
        jwt_claims,
    };

    // ── Dispatch ──────────────────────────────────────────────────────────
    let mut response = match state.runtime.execute(req).await {
        Ok(exec_resp) => {
            let status = StatusCode::from_u16(exec_resp.status)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(exec_resp.body)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error":   "runtime_unreachable",
                "message": e,
            })),
        ).into_response(),
    };

    // Always echo x-request-id back so callers can run `flux trace <id>`.
    if let Ok(val) = request_id.parse::<HeaderValue>() {
        response.headers_mut().insert("x-request-id", val);
    }

    response
}

