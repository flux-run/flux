/// Transparent reverse-proxy for Data-Engine management (control-plane) routes.
///
/// Management traffic (CRUD for databases, tables, schemas, policies, hooks,
/// workflows, cron, subscriptions, relationships) flows:
///   Browser → API → Data Engine
///
/// The API service has already verified the Firebase JWT and resolved
/// tenant/project scope by the time execution reaches this handler.
/// Those headers are forwarded as-is so the data-engine can apply its
/// own tenant-scoped logic.
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
        ) {
            if let Ok(v) = value.to_str() {
                rb = rb.header(name.as_str(), v);
            }
        }
    }

    // Internal-service token so data-engine can trust the call.
    let token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| "fluxbase_secret_token".to_string());
    rb = rb.header("x-service-token", token);

    // Inject resolved tenant/project context headers that the data-engine
    // expects (x-tenant-id, x-project-id, x-tenant-slug, x-project-slug).
    if let Some(tid) = context.tenant_id {
        rb = rb.header("x-tenant-id", tid.to_string());
    }
    if let Some(pid) = context.project_id {
        rb = rb.header("x-project-id", pid.to_string());
    }
    if let Some(ref slug) = context.tenant_slug {
        rb = rb.header("x-tenant-slug", slug.as_str());
    }
    if let Some(ref slug) = context.project_slug {
        rb = rb.header("x-project-slug", slug.as_str());
    }
    rb = rb.header("x-user-id", context.user_id.to_string());
    rb = rb.header("x-user-role", context.role.as_deref().unwrap_or("authenticated"));

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
