use axum::{
    http::StatusCode,
    response::IntoResponse,
    Json,
};

// Represents `/functions`, `/workflows`, etc mentioned in the prompt.
// These are external services but we simulate their gateway entry via the control plane scope check.
pub async fn deploy_function() -> impl IntoResponse {
    Json(serde_json::json!({ "message": "function_deployed" }))
}

/// Execution-plane guard.
///
/// The API service (`api.fluxbase.co`) is the **control plane** only.
/// Function invocation, webhooks, agent execution, and all runtime traffic
/// must flow through the Gateway (`{tenant_slug}.fluxbase.co`).
///
/// Any request that reaches a well-known execution path on this domain is
/// rejected here explicitly so that architectural drift is caught at runtime,
/// not discovered in a post-mortem.
pub async fn execution_not_allowed(
    req: axum::extract::Request,
) -> impl IntoResponse {
    tracing::warn!(
        "execution_plane_misroute: {} {} — should target {{tenant_slug}}.fluxbase.co",
        req.method(),
        req.uri().path(),
    );
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(serde_json::json!({
            "error": "execution_not_allowed_on_control_plane",
            "message": "Function execution must go through the Gateway. Use https://{tenant_slug}.fluxbase.co/{function_name}",
            "docs": "https://docs.fluxbase.co/concepts#execution-plane"
        })),
    )
}
