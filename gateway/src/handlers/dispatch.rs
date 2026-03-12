//! Function-invocation dispatch — the main request pipeline.
//!
//! Registers on `/{*path}` and owns the full lifecycle of a function call:
//!
//! ```text
//!  Request
//!    │
//!    ▼  [1] content-length guard
//!    ▼  [2] route resolution      (in-memory snapshot)
//!    ▼  [3] CORS preflight        (OPTIONS fast path)
//!    ▼  [4] authentication        (none | api_key | jwt)
//!    ▼  [5] rate limiting         (per-route token bucket)
//!    ▼  [6] read + validate body
//!    ▼  [7] write trace root      (fire-and-forget)
//!    ▼  [8] forward to runtime    (POST /execute)
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

    // ── [1] Content-Length guard ─────────────────────────────────────────────
    if let Some(resp) = check_content_length(&headers, state.max_request_size_bytes) {
        return resp;
    }

    // ── [2] Route resolution ─────────────────────────────────────────────────
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
    if method == Method::OPTIONS && route.cors_enabled {
        return cors_preflight(&route);
    }

    // ── [4] Authentication ───────────────────────────────────────────────────
    let auth_ctx = if state.local_mode {
        auth::AuthContext::Dev
    } else {
        match auth::check(&state.db_pool, &state.jwks_cache, &headers, &route).await {
            Ok(ctx)    => ctx,
            Err(msg)   => return unauthorized(&msg),
        }
    };

    // ── [5] Rate limiting ────────────────────────────────────────────────────
    let limit = route.rate_limit
        .map(|r| r.max(0) as u32)
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
        payload.clone(),
    );

    // ── [8] Forward to runtime ───────────────────────────────────────────────
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
