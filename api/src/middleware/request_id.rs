/// Request ID middleware.
///
/// Assigns every request a unique `x-request-id` (UUID v4) unless the caller
/// already supplied one (e.g. from a Cloudflare Worker or client retry).
/// The ID is:
///   • Injected into the request headers so downstream calls (Data Engine,
///     Runtime, etc.) can propagate it through their own logs.
///   • Echoed back in the response headers so clients can correlate errors.
///   • Logged at request start (INFO) and completion (INFO with latency/status).
///
/// Resulting Cloud Run log shape:
///   {"request_id":"abc123","method":"GET","path":"/schema/graph","msg":"request started"}
///   {"request_id":"abc123","status":200,"latency_ms":34,"msg":"request completed"}
use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use uuid::Uuid;

pub async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    // Reuse an incoming ID so the trace is consistent across retries / proxies.
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Propagate into the request so forward_headers() picks it up automatically.
    if let Ok(val) = request_id.parse() {
        req.headers_mut().insert("x-request-id", val);
    }

    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let start = std::time::Instant::now();

    // Skip noisy health-check logging.
    let is_health = path == "/health" || path == "/version";
    if !is_health {
        tracing::info!(
            request_id = %request_id,
            method     = %method,
            path       = %path,
            "request started",
        );
    }

    let mut resp = next.run(req).await;

    if !is_health {
        let latency_ms = start.elapsed().as_millis();
        let status = resp.status().as_u16();
        tracing::info!(
            request_id = %request_id,
            status,
            latency_ms,
            "request completed",
        );
    }

    // Echo ID back so clients can correlate errors immediately.
    if let Ok(val) = request_id.parse() {
        resp.headers_mut().insert("x-request-id", val);
    }

    // Inject request_id into error response bodies so developers can run
    // `flux trace <request_id>` directly from any error message.
    if resp.status().is_client_error() || resp.status().is_server_error() {
        let is_json = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map_or(false, |ct| ct.contains("application/json"));

        if is_json {
            let (parts, body) = resp.into_parts();
            // Drain body — unwrap_or_default ensures resp is always reassigned below.
            let bytes = axum::body::to_bytes(body, 512 * 1024).await
                .unwrap_or_default();
            let new_body = if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if json.get("success") == Some(&serde_json::json!(false)) {
                    json["request_id"] = serde_json::json!(&request_id);
                }
                serde_json::to_vec(&json).unwrap_or_else(|_| bytes.to_vec())
            } else {
                bytes.to_vec()
            };
            resp = Response::from_parts(parts, Body::from(new_body));
        }
    }

    resp
}
