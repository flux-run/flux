/// Transparent reverse-proxy for Data-Engine management (control-plane) routes.
///
/// Management traffic (CRUD for databases, tables, schemas, policies, hooks,
/// workflows, cron, subscriptions, relationships) flows:
///   Browser / CLI → API → Data Engine
///
/// The API service resolves tenant/project scope in middleware before forwarding.
/// Those IDs are injected as x-tenant-id / x-project-id so the data-engine can
/// apply its own tenant-scoped logic.
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use crate::{AppState, types::context::RequestContext};

pub async fn proxy_handler(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    req: Request,
) -> Result<Response, StatusCode> {
    let method     = req.method().clone();
    let uri        = req.uri().clone();
    let in_headers = req.headers().clone();

    // Handle CORS preflight — the outer CorsLayer normally does this, but
    // belt-and-suspenders for direct forwarding.
    if method == axum::http::Method::OPTIONS {
        return Ok((StatusCode::NO_CONTENT, "").into_response());
    }

    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());

    let target_url = format!("{}{}", state.data_engine_url, path_and_query);

    // Read body.
    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Build forwarded request.
    let mut rb = state
        .http_client
        .request(
            reqwest::Method::from_bytes(method.as_str().as_bytes())
                .map_err(|_| StatusCode::BAD_REQUEST)?,
            &target_url,
        );

    // Pass through auth and context headers.
    for (name, value) in in_headers.iter() {
        let n = name.as_str().to_lowercase();
        if matches!(
            n.as_str(),
            "authorization"
                | "content-type"
                | "accept"
                | "x-fluxbase-tenant"
                | "x-fluxbase-project"
                | "x-flux-replay"
        ) {
            if let Ok(v) = value.to_str() {
                rb = rb.header(name.as_str(), v);
            }
        }
    }

    // Internal-service token so data-engine can trust the call.
    let token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| {
            if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                     The API service cannot start without it."
                );
            }
            tracing::warn!(
                "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'fluxbase_secret_token'. \
                 Set INTERNAL_SERVICE_TOKEN in production."
            );
            "fluxbase_secret_token".to_string()
        });
    rb = rb.header("x-service-token", token);

    // Inject resolved tenant/project context headers for the data-engine.
    rb = rb.header("x-tenant-id", context.tenant_id.to_string());
    rb = rb.header("x-project-id", context.project_id.to_string());

    if !body_bytes.is_empty() {
        rb = rb.body(body_bytes.to_vec());
    }

    // Forward and stream response back.
    let upstream = rb.send().await.map_err(|e| {
        tracing::error!("data-engine management proxy error: {:?}", e);
        StatusCode::BAD_GATEWAY
    })?;

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let resp_bytes = upstream.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let response = axum::response::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(axum::body::Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(response)
}
