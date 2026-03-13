//! Runtime forwarding — function invocation dispatch.
//!
//! ## What `to_runtime` does
//!
//! [`to_runtime`] translates a validated, authenticated gateway request into
//! an [`ExecuteRequest`] and sends it to the Runtime service via the
//! [`RuntimeDispatch`] trait.  It returns the runtime's response verbatim
//! (status code + body) so the gateway acts as a transparent proxy.
//!
//! ## Why `x-request-id` is echoed back
//!
//! The gateway always injects `x-request-id` into the response, even when
//! the runtime does not set it.  This lets callers run `flux trace <id>` with
//! the ID from *any* response — the trace root row is guaranteed to exist in
//! `gateway_trace_requests` under that ID.
//!
//! ## Gateway ↔ Runtime contract
//!
//! The gateway communicates intent via headers, not URL construction:
//!
//! | Header                | Direction  | Purpose                              |
//! |-----------------------|------------|--------------------------------------|
//! | `X-Service-Token`     | → runtime  | inter-service authentication         |
//! | `x-request-id`        | → runtime  | trace correlation                    |
//! | `x-parent-span-id`    | → runtime  | nested span linkage                  |
//! | `X-Function-Runtime`  | → runtime  | hint: `"deno"` or `"wasm"`           |
//! | `X-User-Id`           | → runtime  | authenticated user identity          |
//! | `X-JWT-Claims`        | → runtime  | full JWT claims as JSON string       |
//! | `x-request-id`        | ← response | echoed for caller traceability       |
//!
//! The runtime must treat `X-Service-Token` as a shared secret; requests
//! without a valid token should be rejected at the runtime boundary.

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

