/// Realtime SSE proxy.
///
/// GET /events/stream  — proxy the SSE subscription to the Fluxbase API,
///                       forwarding auth headers and query parameters.
///
/// This lets SDK clients connect to the gateway on port 8081 and receive
/// realtime table-change events without needing direct access to the API.
/// The gateway forwards the long-lived SSE connection transparently.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::state::SharedState;

/// Proxy `GET /events/stream` to the upstream API and stream back the SSE body.
pub async fn stream(
    State(state): State<SharedState>,
    req: Request<Body>,
) -> Response {
    // Build the upstream URL, preserving the query string.
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let upstream = format!("{}/events/stream{}", state.api_url, query);

    // Forward a safe subset of headers to the API.
    // We need Authorization (Bearer token or API key) and the Fluxbase scope
    // headers so the project-scope middleware on the API can authenticate the
    // request.
    let mut forward_headers = reqwest::header::HeaderMap::new();
    for name in &[
        header::AUTHORIZATION,
        header::ACCEPT,
        "x-fluxbase-tenant".parse().unwrap(),
        "x-fluxbase-project".parse().unwrap(),
        "x-api-key".parse().unwrap(),
    ] {
        if let Some(value) = req.headers().get(name) {
            if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                forward_headers.insert(name.clone(), v);
            }
        }
    }
    // Always request SSE content type.
    forward_headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("text/event-stream"),
    );

    // Execute the upstream request.
    let upstream_res = match state
        .http_client
        .get(&upstream)
        .headers(forward_headers)
        .send()
        .await
    {
        Ok(r)  => r,
        Err(e) => {
            tracing::error!("SSE proxy upstream error: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": "sse_proxy_upstream_error",
                    "message": e.to_string()
                })),
            )
                .into_response();
        }
    };

    // Propagate non-2xx status codes directly.
    let status = StatusCode::from_u16(upstream_res.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);

    if !status.is_success() {
        let body = upstream_res.text().await.unwrap_or_default();
        return (status, body).into_response();
    }

    // Stream the SSE bytes back to the client.
    let byte_stream = upstream_res.bytes_stream();
    let axum_body  = Body::from_stream(byte_stream);

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    response_headers.insert(
        header::CONNECTION,
        HeaderValue::from_static("keep-alive"),
    );
    // Allow the browser to connect from any origin — the API's CORS policy
    // already governs the API requests; here we just need to not block SSE.
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );

    (status, response_headers, axum_body).into_response()
}
