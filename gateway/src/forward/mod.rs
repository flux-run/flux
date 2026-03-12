//! Runtime forwarding.
//!
//! Sends a `POST /execute` to the Runtime service and streams the response
//! back to the caller.  Adds tracing and auth-context headers so the runtime
//! can identify the function and user without re-fetching metadata.
use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use crate::auth::AuthContext;
use crate::snapshot::RouteRecord;
use crate::state::SharedState;

/// Forward a function-invocation request to the runtime.
///
/// Returns the runtime's response verbatim, with `x-request-id` echoed back.
pub async fn to_runtime(
    state:      &SharedState,
    route:      &RouteRecord,
    payload:    Value,
    request_id: &str,
    parent_span: Option<&str>,
    auth_ctx:   &AuthContext,
) -> Response {
    let url = format!("{}/execute", state.runtime_url);

    let body = serde_json::json!({
        "function_id": route.function_id.to_string(),
        "project_id":  route.project_id.to_string(),
        "payload":     payload,
    });

    let mut builder = state.http_client
        .post(&url)
        .header("X-Service-Token",    &state.internal_service_token)
        .header("X-Function-Runtime", &route.runtime)
        .header("x-request-id",       request_id)
        .json(&body);

    // Forward auth context so functions know who the caller is.
    match auth_ctx {
        AuthContext::Jwt { user_id, claims } => {
            if let Some(uid) = user_id {
                builder = builder.header("X-User-Id", uid.as_str());
            }
            if let Some(c) = claims {
                if let Ok(json) = serde_json::to_string(c) {
                    builder = builder.header("X-JWT-Claims", json);
                }
            }
        }
        _ => {}
    }

    if let Some(span) = parent_span {
        builder = builder.header("x-parent-span-id", span);
    }

    let mut response = match builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            let raw    = resp.text().await.unwrap_or_default();
            let body: Value = serde_json::from_str(&raw).unwrap_or_else(|_| {
                tracing::warn!(
                    status = %status,
                    preview = %&raw[..raw.len().min(200)],
                    "Runtime returned non-JSON body"
                );
                serde_json::json!({ "error": "runtime_response_parse_error" })
            });
            (status, Json(body)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error":   "runtime_unreachable",
                "message": e.to_string(),
            })),
        ).into_response(),
    };

    // Always echo x-request-id back so callers can run `flux trace <id>`.
    if let Ok(val) = request_id.parse::<HeaderValue>() {
        response.headers_mut().insert("x-request-id", val);
    }

    response
}
