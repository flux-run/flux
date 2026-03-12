//! Tenant routing — the core request-dispatch pipeline.
//!
//! This module owns the full lifecycle of an inbound tenant function call:
//!
//! ```text
//!  HTTP request
//!      │
//!      ▼
//!  [0] resolve identity  (tenant / project from hostname extension)
//!      │
//!      ▼
//!  [1] match route       (in-memory snapshot lookup)
//!      │
//!      ├─ OPTIONS → CORS preflight fast-path
//!      │
//!      ▼
//!  [2] authenticate      (none | api_key | jwt)
//!      │
//!      ▼
//!  [3] rate-limit        (token-bucket per route×IP)
//!      │
//!      ▼
//!  [3.5] schema-validate (JSON Schema if configured)
//!      │
//!      ├─ is_async=true → enqueue job → 202 Accepted
//!      │
//!      ▼
//!  [4] dispatch to runtime  (POST /execute, runtime-aware headers)
//!      │
//!      ▼
//!  [5] apply CORS + trace headers → return response
//! ```
//!
//! The runtime type (`deno` | `wasm`) is carried in the route snapshot and
//! forwarded via the `X-Function-Runtime` header so the runtime service can
//! dispatch immediately without re-fetching bundle metadata.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use job_contract::job::CreateJobRequest;
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use crate::state::SharedState;

/// Entry-point for all tenant function requests.
///
/// Registered on `/{*path}` behind the `identity_resolver` middleware.
pub async fn tenant_route_handler(
    State(state): State<SharedState>,
    method: Method,
    Path(path): Path<String>,
    req: Request<Body>,
) -> Response {
    let full_path = format!("/{}", path);
    let method_str = method.to_string();
    let start_time = std::time::Instant::now();

    // ── [0] Resolve identity ──────────────────────────────────────────────────
    let (tenant_id, tenant_slug) =
        match req.extensions().get::<crate::middleware::identity_resolver::ResolvedIdentity>() {
            Some(id) => (id.tenant_id, id.tenant_slug.clone()),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "identity_resolution_failed",
                        "message": "Could not resolve project/tenant identity from host"
                    })),
                )
                    .into_response();
            }
        };

    // ── Distributed tracing — extract / synthesise request & span IDs ────────
    //
    // Fallback chain:
    //   request_id:  x-request-id → W3C traceparent trace_id → new UUID
    //   parent_span: x-parent-span-id → W3C traceparent parent_id → None
    let traceparent = req
        .headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(crate::middleware::traceparent::parse);

    let incoming_request_id: String = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| traceparent.as_ref().map(|tp| tp.trace_id.clone()))
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let incoming_parent_span_id: Option<String> = req
        .headers()
        .get("x-parent-span-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| traceparent.map(|tp| tp.parent_id));

    // ── [1] Resolve route from in-memory snapshot ─────────────────────────────
    let snapshot_data = state.snapshot.get_data().await;
    if snapshot_data.routes.is_empty()
        && std::env::var("SKIP_SNAPSHOT_READY_CHECK").is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "gateway_not_ready",
                "message": "Gateway is loading routing configuration. Please retry in a few seconds."
            })),
        )
            .into_response();
    }

    let cache_key = (tenant_id, method_str.clone(), full_path.clone());

    let route = match snapshot_data.routes.get(&cache_key) {
        Some(r) => {
            tracing::debug!(
                tenant_id = %tenant_id,
                runtime   = %r.runtime,
                "Route cache hit: {} {}",
                method_str,
                full_path
            );
            r.clone()
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "route_not_found",
                    "message": format!("No route for {} {}", method_str, full_path)
                })),
            )
                .into_response();
        }
    };

    // ── Automatic span: gateway routing ──────────────────────────────────────
    {
        let pool        = state.db_pool.clone();
        let tid         = tenant_id;
        let pid         = route.project_id;
        let fid         = route.function_id.to_string();
        let rid         = incoming_request_id.clone();
        let runtime     = route.runtime.clone();
        let msg         = format!("route matched [{runtime}]: {} {}", method_str, full_path);
        let parent_span = incoming_parent_span_id.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO platform_logs \
                 (id, tenant_id, project_id, source, resource_id, level, message, request_id, span_type, parent_span_id) \
                 VALUES ($1, $2, $3, 'gateway', $4, 'info', $5, $6, 'start', $7)",
            )
            .bind(Uuid::new_v4())
            .bind(tid)
            .bind(pid)
            .bind(fid)
            .bind(msg)
            .bind(rid)
            .bind(parent_span.as_ref().and_then(|s| Uuid::parse_str(s).ok()))
            .execute(&pool)
            .await;
        });
    }

    // ── [1.5] CORS preflight fast-path ────────────────────────────────────────
    if method == Method::OPTIONS && route.cors_enabled {
        let mut response =
            (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response();
        inject_cors_headers(response.headers_mut(), &route);
        return response;
    }

    // ── [2] Authenticate ──────────────────────────────────────────────────────
    let mut fwd_user_id: Option<String> = None;
    let mut fwd_jwt_claims: Option<String> = None;

    if route.auth_type == "api_key" {
        let api_key = req.headers().get("X-API-Key").and_then(|h| h.to_str().ok());
        match api_key {
            Some(key) => {
                match crate::middleware::auth::validate_api_key(&state.db_pool, key).await {
                    Ok(true) => {}
                    Ok(false) => {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({
                                "error": "invalid_api_key",
                                "message": "The provided API key is invalid or revoked"
                            })),
                        )
                            .into_response();
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "error": "auth_error",
                                "message": e.to_string()
                            })),
                        )
                            .into_response();
                    }
                }
            }
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": "missing_api_key",
                        "message": "X-API-Key header is required for this route"
                    })),
                )
                    .into_response();
            }
        }
    } else if route.auth_type == "jwt" {
        let jwks_url = match &route.jwks_url {
            Some(u) => u,
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "auth_config_error",
                        "message": "Route requires JWT but no JWKS URL configured"
                    })),
                )
                    .into_response();
            }
        };

        match crate::middleware::jwt_auth::verify_jwt(
            req.headers(),
            jwks_url,
            route.jwt_audience.as_deref(),
            route.jwt_issuer.as_deref(),
            &state.jwks_cache,
        )
        .await
        {
            Ok(claims) => {
                if let Some(uid) = claims.user_id.or(claims.sub.clone()) {
                    fwd_user_id = Some(uid);
                }
                if let Ok(claims_str) = serde_json::to_string(&claims.custom) {
                    fwd_jwt_claims = Some(claims_str);
                }
            }
            Err(e) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "invalid_jwt", "message": e })),
                )
                    .into_response();
            }
        }
    }

    // ── [3] Rate-limit ────────────────────────────────────────────────────────
    if let Some(limit) = route.rate_limit {
        let client_ip = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");

        let limit_key = format!("{}:{}", route.id, client_ip);
        if !crate::middleware::rate_limit::check_rate_limit(&limit_key, limit.max(0) as u32) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "rate_limit_exceeded",
                    "message": "Too many requests. Please try again later."
                })),
            )
                .into_response();
        }
    }

    // ── Body / header extraction ──────────────────────────────────────────────
    let max_request_size = std::env::var("MAX_REQUEST_SIZE_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10 * 1024 * 1024); // 10 MB

    // Fail fast on Content-Length before reading the body.
    if let Some(cl) = req.headers().get("content-length") {
        if let Ok(n) = cl.to_str().unwrap_or("0").parse::<usize>() {
            if n > max_request_size {
                return payload_too_large(max_request_size);
            }
        }
    }

    // Snapshot redacted headers for trace logging.
    let mut headers_map = HashMap::new();
    for (key, value) in req.headers() {
        let redacted = match key.as_str().to_lowercase().as_str() {
            "authorization" | "x-api-key" | "cookie" => "[REDACTED]".to_string(),
            _ => value.to_str().unwrap_or("[INVALID_UTF8]").to_string(),
        };
        headers_map.insert(key.to_string(), redacted);
    }

    let idempotency_key = req
        .headers()
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body_bytes = match axum::body::to_bytes(req.into_body(), max_request_size).await {
        Ok(b) => b,
        Err(_) => return payload_too_large(max_request_size),
    };

    let payload: Value =
        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));

    // ── Persist trace_requests envelope (before any processing) ──────────────
    {
        let pool        = state.db_pool.clone();
        let rid         = incoming_request_id.clone();
        let tid         = tenant_id;
        let pid         = route.project_id;
        let fid         = route.function_id;
        let req_method  = method_str.clone();
        let req_path    = full_path.clone();
        let req_headers = serde_json::to_value(&headers_map).ok();
        let req_payload = payload.clone();

        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO trace_requests \
                 (request_id, tenant_id, project_id, function_id, function_version, \
                  method, path, headers, body, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())",
            )
            .bind(&rid)
            .bind(tid)
            .bind(pid)
            .bind(fid)
            .bind("") // captured from runtime deployment info
            .bind(&req_method)
            .bind(&req_path)
            .bind(req_headers)
            .bind(req_payload)
            .execute(&pool)
            .await;
        });
    }

    // ── [3.5] JSON Schema validation ──────────────────────────────────────────
    if let Some(schema_val) = &route.json_schema {
        match jsonschema::validator_for(schema_val) {
            Ok(compiled) => {
                if let Err(error) = compiled.validate(&payload) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "validation_failed",
                            "message": "Request payload failed schema validation",
                            "details": [error.to_string()]
                        })),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                tracing::error!(route_id = %route.id, "Invalid JSON Schema: {:?}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "schema_config_error",
                        "message": "Invalid JSON Schema configured for route"
                    })),
                )
                    .into_response();
            }
        }
    }

    // ── Async dispatch — enqueue job ──────────────────────────────────────────
    if route.is_async {
        let enqueue_req = CreateJobRequest {
            tenant_id:    route.tenant_id,
            project_id:   route.project_id,
            function_id:  route.function_id,
            payload,
            idempotency_key,
        };

        let mut response = match state.queue_client.enqueue(enqueue_req).await {
            Ok(job) => (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({ "job_id": job.job_id, "status": "queued" })),
            )
                .into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "queue_unreachable",
                    "message": e.to_string()
                })),
            )
                .into_response(),
        };

        if route.cors_enabled {
            inject_cors_headers(response.headers_mut(), &route);
        }

        crate::middleware::analytics::log_request(
            &state.metric_tx,
            route.id,
            tenant_id,
            response.status().as_u16(),
            start_time.elapsed().as_millis() as i64,
        );

        return response;
    }

    // ── [4] Dispatch to runtime ───────────────────────────────────────────────
    //
    // `X-Function-Runtime` lets the runtime service skip re-fetching bundle
    // metadata and dispatch directly to the Deno or WASM executor.
    let runtime_url = format!("{}/execute", state.runtime_url);

    let forward_payload = serde_json::json!({
        "function_id": route.function_id.to_string(),
        "tenant_id":   route.tenant_id.to_string(),
        "project_id":  route.project_id.to_string(),
        "payload":     payload,
    });

    let mut req_builder = state
        .http_client
        .post(&runtime_url)
        .header("X-Service-Token",      &state.internal_service_token)
        .header("X-Tenant-Id",          tenant_id.to_string())
        .header("X-Tenant-Slug",        &tenant_slug)
        .header("X-Function-Runtime",   &route.runtime)
        .header("x-request-id",         &incoming_request_id)
        .json(&forward_payload);

    if let Some(uid) = fwd_user_id {
        req_builder = req_builder.header("X-User-Id", uid);
    }
    if let Some(claims) = fwd_jwt_claims {
        req_builder = req_builder.header("X-JWT-Claims", claims);
    }
    if let Some(parent_span) = &incoming_parent_span_id {
        req_builder = req_builder.header("x-parent-span-id", parent_span);
    }

    // ── [5] Build response, inject CORS + trace headers ───────────────────────
    let mut response = match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            let raw = resp.text().await.unwrap_or_default();
            let body: Value = serde_json::from_str(&raw).unwrap_or_else(|_| {
                tracing::warn!(
                    status       = %status,
                    raw_body_preview = %&raw[..raw.len().min(200)],
                    "runtime returned non-JSON"
                );
                serde_json::json!({ "error": "runtime_response_parse_error" })
            });
            (status, Json(body)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": "runtime_unreachable",
                "message": e.to_string()
            })),
        )
            .into_response(),
    };

    // Always echo x-request-id back so callers can run `flux trace <id>`.
    if let Ok(val) = incoming_request_id.parse::<HeaderValue>() {
        response.headers_mut().insert("x-request-id", val);
    }
    if let Some(parent_span) = &incoming_parent_span_id {
        if let Ok(val) = parent_span.parse::<HeaderValue>() {
            response.headers_mut().insert("x-parent-span-id", val);
        }
    }
    if route.cors_enabled {
        inject_cors_headers(response.headers_mut(), &route);
    }

    crate::middleware::analytics::log_request(
        &state.metric_tx,
        route.id,
        tenant_id,
        response.status().as_u16(),
        start_time.elapsed().as_millis() as i64,
    );

    response
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Inject CORS response headers derived from the route configuration.
fn inject_cors_headers(
    headers: &mut axum::http::HeaderMap,
    route: &crate::services::route_lookup::RouteRecord,
) {
    let origin = route
        .cors_origins
        .as_ref()
        .and_then(|o| if o.is_empty() { None } else { Some(o.join(", ")) })
        .unwrap_or_else(|| "*".to_string());

    let allowed_headers = route
        .cors_headers
        .as_ref()
        .and_then(|h| if h.is_empty() { None } else { Some(h.join(", ")) })
        .unwrap_or_else(|| "Content-Type, Authorization, X-API-Key".to_string());

    if let Ok(val) = origin.parse() {
        headers.insert("Access-Control-Allow-Origin", val);
    }
    if let Ok(val) = "GET, POST, PUT, DELETE, OPTIONS".parse() {
        headers.insert("Access-Control-Allow-Methods", val);
    }
    if let Ok(val) = allowed_headers.parse() {
        headers.insert("Access-Control-Allow-Headers", val);
    }
}

/// 413 Payload Too Large response helper.
fn payload_too_large(limit: usize) -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(serde_json::json!({
            "error":   "payload_too_large",
            "message": format!("Request body exceeds {} bytes limit", limit)
        })),
    )
        .into_response()
}
