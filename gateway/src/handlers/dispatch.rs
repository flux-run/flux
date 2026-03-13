//! Function-invocation dispatch — the main request pipeline.
//!
//! Registers on `/{*path}` and owns the full lifecycle of a function call:
//!
//! ```text
//!  Request
//!    │
//!    ▼  [1] content-length guard   — fast-reject bodies above the byte limit
//!    ▼                               before reading any bytes
//!    ▼  [2] route resolution       — (METHOD, /path) lookup in the in-memory
//!    ▼                               snapshot; returns 404 when unknown
//!    ▼  [3] CORS preflight         — OPTIONS fast-path; injects CORS headers
//!    ▼                               and returns 204 without auth
//!    ▼  [4] authentication         — dispatches to none/api_key/jwt based on
//!    ▼                               the route's auth_type; 401 on failure
//!    ▼  [5] rate limiting          — per-route token bucket keyed on
//!    ▼                               route_id × client_ip; 429 when exhausted
//!    ▼  [6] read + validate body   — stream and buffer the body; optionally
//!    ▼                               run JSON Schema validation (route config)
//!    ▼  [7] write trace root       — fire-and-forget DB write; captures path,
//!    ▼                               headers (credentials redacted),
//!    ▼                               query_params, and body for `flux trace`
//!    ▼  [8] forward to runtime     — POST /execute via RuntimeDispatch trait;
//!    ▼                               auth-context threaded as structured fields
//!    ▼
//!  Response
//! ```
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::collections::HashMap;
use crate::{auth, forward, rate_limit, snapshot::RouteRecord, state::SharedState, trace};

pub async fn handle(
    State(state): State<SharedState>,
    method:       Method,
    Path(path):   Path<String>,
    req:          Request<Body>,
) -> Response {
    let path        = format!("/{}", path);
    let method_str  = method.as_str().to_uppercase();
    let headers     = req.headers().clone();

    // Extract query params here, before `req.into_body()` consumes the request.
    // Stored in the trace root so `flux trace` can show the full original URL.
    let query_params: HashMap<String, String> = req.uri().query()
        .map(|q| {
            q.split('&')
             .filter_map(|pair| {
                 let mut parts = pair.splitn(2, '=');
                 let k = parts.next()?.to_string();
                 let v = parts.next().unwrap_or("").to_string();
                 Some((k, v))
             })
             .collect()
        })
        .unwrap_or_default();

    // ── [1] Content-Length guard ─────────────────────────────────────────────
    // Reject before reading any bytes — avoids holding a connection open while
    // streaming a giant body that we would ultimately discard.
    if let Some(resp) = check_content_length(&headers, state.max_request_size_bytes) {
        return resp;
    }

    // ── [2] Route resolution ─────────────────────────────────────────────────
    // All routing is done against the in-memory snapshot — zero DB reads on
    // the hot path.  The snapshot is kept fresh via Postgres LISTEN/NOTIFY.
    let snapshot = state.snapshot.get_data().await;

    if snapshot.routes.is_empty()
        && std::env::var("SKIP_SNAPSHOT_READY_CHECK").is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error":   "gateway_not_ready",
                "message": "Route snapshot is loading — retry in a moment",
            })),
        ).into_response();
    }

    let route = match snapshot.routes.get(&(method_str.clone(), path.clone())) {
        Some(r) => r.clone(),
        None => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error":   "route_not_found",
                "message": format!("{} {} is not registered", method_str, path),
            })),
        ).into_response(),
    };

    // ── [3] CORS preflight fast path ─────────────────────────────────────────
    // OPTIONS must be answered before authentication — browsers send preflight
    // without credentials, so requiring auth here would break cross-origin calls.
    if method == Method::OPTIONS && route.cors_enabled {
        return cors_preflight(&route);
    }

    // ── [4] Authentication ───────────────────────────────────────────────────
    // Strategy is determined by the route's auth_type field, not by a global
    // policy.  local_mode skips auth entirely for `flux dev` development stacks.
    let auth_ctx = if state.local_mode {
        auth::AuthContext::Dev
    } else {
        match auth::check(&state.db_pool, &state.jwks_cache, &headers, &route).await {
            Ok(ctx)    => ctx,
            Err(msg)   => return unauthorized(&msg),
        }
    };

    // ── [5] Rate limiting ────────────────────────────────────────────────────
    // Keyed on route_id × client_ip so each function gets an independent
    // per-caller counter.  The route-level limit overrides the global default
    // when configured; negative DB values are clamped to zero.
    let limit = route.rate_limit_per_minute
        .map(|r| r.max(0) as u32 / 60)
        .unwrap_or(state.rate_limit_per_sec);

    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    if !rate_limit::allow(&rate_limit::key(route.id, client_ip), limit) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error":   "rate_limit_exceeded",
                "message": "Too many requests — please slow down",
            })),
        ).into_response();
    }

    // ── [6] Read body ────────────────────────────────────────────────────────
    // Body is streamed into memory now (the request is consumed here).
    // A second size check catches chunked transfers that bypassed stage [1].
    // Schema validation is deferred until the body is fully buffered to avoid
    // partial-read confusion on validation failure.
    let body_bytes = match axum::body::to_bytes(req.into_body(), state.max_request_size_bytes).await {
        Ok(b)  => b,
        Err(_) => return payload_too_large(state.max_request_size_bytes),
    };

    let payload: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));

    // JSON Schema validation (optional — only when configured on the route).
    if let Some(schema_val) = &route.json_schema {
        if let Ok(validator) = jsonschema::validator_for(schema_val) {
            if let Err(e) = validator.validate(&payload) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error":   "validation_failed",
                        "message": "Request body failed schema validation",
                        "detail":  e.to_string(),
                    })),
                ).into_response();
            }
        }
    }

    // ── [7] Trace root (fire-and-forget) ─────────────────────────────────────
    // Resolve the request ID (from headers or a fresh UUID) and spawn the DB
    // write without awaiting it — the trace write must never delay the caller.
    // Sensitive headers (authorization, x-api-key, cookie) are redacted here
    // so credentials are never stored in gateway_trace_requests.
    let request_id  = trace::resolve_request_id(&headers);
    let parent_span = trace::resolve_parent_span(&headers);

    let redacted_headers: HashMap<String, String> = headers.iter()
        .map(|(k, v)| {
            let val = match k.as_str().to_lowercase().as_str() {
                "authorization" | "x-api-key" | "cookie" => "[REDACTED]".into(),
                _ => v.to_str().unwrap_or("[INVALID_UTF8]").to_string(),
            };
            (k.to_string(), val)
        })
        .collect();

    trace::write_root(
        state.db_pool.clone(),
        &route,
        &request_id,
        &method_str,
        &path,
        redacted_headers,
        query_params,
        payload.clone(),
    );

    // ── [8] Forward to runtime ───────────────────────────────────────────────
    // Gateway depends on the RuntimeDispatch trait, never on the concrete HTTP
    // impl — so the server crate can swap in an in-process implementation.
    // Auth-context is threaded through as structured fields, not raw strings.
    let mut response = forward::to_runtime(
        &state,
        &route,
        payload,
        &request_id,
        parent_span.as_deref(),
        &auth_ctx,
    ).await;

    // Inject CORS headers when enabled for this route.
    if route.cors_enabled {
        inject_cors_headers(response.headers_mut(), &route);
    }

    response
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn check_content_length(headers: &axum::http::HeaderMap, limit: usize) -> Option<Response> {
    if let Some(cl) = headers.get("content-length") {
        if let Ok(n) = cl.to_str().unwrap_or("0").parse::<usize>() {
            if n > limit {
                return Some(payload_too_large(limit));
            }
        }
    }
    None
}

fn payload_too_large(limit: usize) -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(serde_json::json!({
            "error":   "payload_too_large",
            "message": format!("Request body exceeds {} byte limit", limit),
        })),
    ).into_response()
}

fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthorized", "message": msg })),
    ).into_response()
}

fn cors_preflight(route: &RouteRecord) -> Response {
    let mut response =
        (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response();
    inject_cors_headers(response.headers_mut(), route);
    response
}

fn inject_cors_headers(headers: &mut axum::http::HeaderMap, route: &RouteRecord) {
    let origin = route.cors_origins.as_ref()
        .filter(|o| !o.is_empty())
        .map(|o| o.join(", "))
        .unwrap_or_else(|| "*".to_string());

    let allowed_headers = route.cors_headers.as_ref()
        .filter(|h| !h.is_empty())
        .map(|h| h.join(", "))
        .unwrap_or_else(|| "Content-Type, Authorization, X-API-Key".to_string());

    for (name, val) in [
        ("Access-Control-Allow-Origin",  origin.as_str()),
        ("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS"),
        ("Access-Control-Allow-Headers", allowed_headers.as_str()),
    ] {
        if let Ok(v) = val.parse() {
            headers.insert(name, v);
        }
    }
}
