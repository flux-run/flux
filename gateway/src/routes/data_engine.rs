/// Transparent reverse-proxy for Data-Engine execution routes.
///
/// Execution traffic (POST /db/query, cron triggers, file operations) flows:
///   Browser → Gateway → Data Engine
///
/// The gateway validates nothing here beyond forwarding auth headers –
/// the data-engine enforces tenant/project context itself via
/// `X-Fluxbase-Tenant` / `X-Fluxbase-Project`.
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use crate::state::SharedState;

const CORS_ORIGIN:  &str = "*";
const CORS_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const CORS_HEADERS: &str = "Authorization, Content-Type, Accept, X-Fluxbase-Tenant, X-Fluxbase-Project";

fn cors_headers() -> HeaderMap {
    let mut m = HeaderMap::new();
    m.insert("access-control-allow-origin",  HeaderValue::from_static(CORS_ORIGIN));
    m.insert("access-control-allow-methods", HeaderValue::from_static(CORS_METHODS));
    m.insert("access-control-allow-headers", HeaderValue::from_static(CORS_HEADERS));
    m
}

pub async fn proxy_handler(
    State(state): State<SharedState>,
    req: Request,
) -> Result<Response, StatusCode> {
    let method   = req.method().clone();
    let uri      = req.uri().clone();
    let in_headers = req.headers().clone();

    // Fast-path: handle CORS preflight immediately.
    if method == axum::http::Method::OPTIONS {
        let mut resp = (StatusCode::NO_CONTENT, "").into_response();
        resp.headers_mut().extend(cors_headers());
        return Ok(resp);
    }

    // Build target URL: keep full path + query string as-is.
    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());

    let target_url = format!("{}{}", state.data_engine_url, path_and_query);

    // Collect body bytes.
    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Build forwarded request, passing through auth + context headers.
    let mut req_builder = state
        .http_client
        .request(
            reqwest::Method::from_bytes(method.as_str().as_bytes())
                .map_err(|_| StatusCode::BAD_REQUEST)?,
            &target_url,
        );

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
                req_builder = req_builder.header(name.as_str(), v);
            }
        }
    }

    req_builder = req_builder.header("x-service-token", &state.internal_service_token);

    // Generate or forward a request-ID so traces correlate across services.
    let request_id = in_headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    req_builder = req_builder.header("x-request-id", &request_id);

    if !body_bytes.is_empty() {
        req_builder = req_builder.body(body_bytes.to_vec());
    }

    // Forward to data-engine.
    let upstream = req_builder.send().await.map_err(|e| {
        tracing::error!("data-engine proxy error: {:?}", e);
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

    let mut response = axum::response::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("x-request-id", &request_id)
        .body(axum::body::Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    response.headers_mut().extend(cors_headers());
    Ok(response)
}
