use axum::{
    extract::{Path, State},
    http::{Method, Request, StatusCode, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use axum::body::Body;
use crate::state::SharedState;
use job_contract::job::CreateJobRequest;
use serde_json::Value;
use uuid::Uuid;
use std::collections::HashMap;

pub async fn proxy_handler(
    State(state): State<SharedState>,
    method: Method,
    Path(path): Path<String>,
    req: Request<Body>,
) -> Response {
    let full_path = format!("/{}", path);
    let method_str = method.to_string();
    let start_time = std::time::Instant::now();

    // 0. Extract Resolved Identity
    let (tenant_id, tenant_slug) = match req.extensions().get::<crate::middleware::identity_resolver::ResolvedIdentity>() {
        Some(id) => (id.tenant_id, id.tenant_slug.clone()),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "identity_resolution_failed", "message": "Could not resolve project/tenant identity from host" }))
            ).into_response();
        }
    };

    // 0.5 Extract x-request-id and x-parent-span-id early for tracing.
    //
    // Fallback chain:
    //   request_id:   x-request-id  →  W3C traceparent trace_id  →  new UUID
    //   parent_span:  x-parent-span-id  →  W3C traceparent parent_id  →  None
    //
    // This means external systems (OTel collectors, browsers, CDNs) that emit
    // the standard `traceparent` header get full distributed-trace propagation
    // without any client-side changes.
    let traceparent = req.headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(crate::middleware::traceparent::parse);

    let incoming_request_id: String = req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| traceparent.as_ref().map(|tp| tp.trace_id.clone()))
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let incoming_parent_span_id: Option<String> = req.headers()
        .get("x-parent-span-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| traceparent.map(|tp| tp.parent_id));

    // 1. Resolve Route from memory snapshot
    // CRITICAL: Snapshot must be ready before routing any traffic.
    // If routes table is empty (e.g., during cold start), return 503 Service Unavailable.
    let snapshot_data = state.snapshot.get_data().await;
    if snapshot_data.routes.is_empty() && !std::env::var("SKIP_SNAPSHOT_READY_CHECK").is_ok() {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "gateway_not_ready", "message": "Gateway is loading routing configuration. Please retry in a few seconds." }))
        ).into_response();
    }

    let cache_key = (tenant_id, method_str.clone(), full_path.clone());

    let route = if let Some(r) = snapshot_data.routes.get(&cache_key) {
        tracing::debug!("Route cache hit: {} {}", method_str, full_path);
        r.clone()
    } else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "route_not_found", "message": format!("No route for {} {}", method_str, full_path) }))
        ).into_response();
    };

    // ── Automatic span: gateway routing ──────────────────────────────────────
    // Fire-and-forget — do not block the hot path.
    {
        let pool   = state.db_pool.clone();
        let tid    = tenant_id;
        let pid    = route.project_id;
        let fid    = route.function_id.to_string();
        let rid    = incoming_request_id.clone();
        let msg    = format!("route matched: {} {}", method_str, full_path);
        let parent_span = incoming_parent_span_id.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO platform_logs \
                 (id, tenant_id, project_id, source, resource_id, level, message, request_id, span_type, parent_span_id) \
                 VALUES ($1, $2, $3, 'gateway', $4, 'info', $5, $6, 'start', $7)"
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

    // 1.5 CORS Preflight Fast-Path
    if method == Method::OPTIONS && route.cors_enabled {
        let mut response = (axum::http::StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response();
        let headers = response.headers_mut();
        
        let origin_str = route.cors_origins
            .as_ref()
            .and_then(|o| if o.is_empty() { None } else { Some(o.join(", ")) })
            .unwrap_or_else(|| "*".to_string());
            
        let allowed_headers = route.cors_headers
            .as_ref()
            .and_then(|h| if h.is_empty() { None } else { Some(h.join(", ")) })
            .unwrap_or_else(|| "Content-Type, Authorization, X-API-Key".to_string());
            
        if let Ok(val) = origin_str.parse() {
            headers.insert("Access-Control-Allow-Origin", val);
        }
        headers.insert("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap());
        if let Ok(val) = allowed_headers.parse() {
            headers.insert("Access-Control-Allow-Headers", val);
        }
        return response;
    }

    // 2. Authenticate
    let mut fwd_user_id: Option<String> = None;
    let mut fwd_jwt_claims: Option<String> = None;

    if route.auth_type == "api_key" {
        let api_key = req.headers().get("X-API-Key")
            .and_then(|h| h.to_str().ok());
        
        match api_key {
            Some(key) => {
                match crate::middleware::auth::validate_api_key(&state.db_pool, key).await {
                    Ok(true) => {}, // Valid
                    Ok(false) => {
                        return (
                            axum::http::StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({ "error": "invalid_api_key", "message": "The provided API key is invalid or revoked" }))
                        ).into_response();
                    }
                    Err(e) => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": "auth_error", "message": e.to_string() }))
                        ).into_response();
                    }
                }
            }
            None => {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "missing_api_key", "message": "X-API-Key header is required for this route" }))
                ).into_response();
            }
        }
    } else if route.auth_type == "jwt" {
        let jwks_url = match &route.jwks_url {
            Some(u) => u,
            None => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "auth_config_error", "message": "Route requires JWT but no JWKS URL configured" }))
                ).into_response();
            }
        };

        match crate::middleware::jwt_auth::verify_jwt(
            req.headers(),
            jwks_url,
            route.jwt_audience.as_deref(),
            route.jwt_issuer.as_deref(),
            &state.jwks_cache
        ).await {
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
                    axum::http::StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "invalid_jwt", "message": e }))
                ).into_response();
            }
        }
    }

    // 3. Rate Limit
    if let Some(limit) = route.rate_limit {
        let client_ip = req.headers().get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown");
        
        let limit_key = format!("{}:{}", route.id, client_ip);
        if !crate::middleware::rate_limit::check_rate_limit(&limit_key, limit.max(0) as u32) {
            return (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "rate_limit_exceeded", "message": "Too many requests. Please try again later." }))
            ).into_response();
        }
    }

    // 4. Forward to Runtime
    let runtime_url = format!("{}/execute", state.runtime_url);

    // Validate Request Size
    let max_request_size = std::env::var("MAX_REQUEST_SIZE_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10 * 1024 * 1024); // 10MB default

    // Check Content-Length header if available (fail fast without reading body)
    if let Some(content_length_header) = req.headers().get("content-length") {
        if let Ok(content_length_str) = content_length_header.to_str() {
            if let Ok(content_length) = content_length_str.parse::<usize>() {
                if content_length > max_request_size {
                    return (
                        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                        Json(serde_json::json!({
                            "error": "payload_too_large",
                            "message": format!("Request body exceeds {} bytes limit", max_request_size)
                        }))
                    ).into_response();
                }
            }
        }
    }

    // 4.5 Extract headers for trace_requests before consuming request
    let mut headers_map = HashMap::new();
    for (key, value) in req.headers() {
        // Redact sensitive headers from trace logs
        let redacted = match key.as_str().to_lowercase().as_str() {
            "authorization" | "x-api-key" | "cookie" => "[REDACTED]".to_string(),
            _ => value.to_str().unwrap_or("[INVALID_UTF8]").to_string(),
        };
        headers_map.insert(key.to_string(), redacted);
    }

    // Extract Idempotency-Key before req is consumed
    let idempotency_key = req.headers()
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    // Extract the payload from the original request.
    // For now, we assume it's JSON.
    let body_bytes = match axum::body::to_bytes(req.into_body(), max_request_size).await {
        Ok(b) => b,
        Err(e) => {
            return (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "error": "payload_too_large",
                    "message": format!("Request body exceeds {} bytes limit", max_request_size)
                }))
            ).into_response();
        }
    };

    let payload: Value = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));

    // CRITICAL: Insert trace_requests envelope NOW (before any processing)
    // This captures the complete request for deterministic replay.
    {
        let pool = state.db_pool.clone();
        let rid = incoming_request_id.clone();
        let tid = tenant_id;
        let pid = route.project_id;
        let fid = route.function_id;
        let req_method = method_str.clone();
        let req_path = full_path.clone();
        let req_headers = serde_json::to_value(&headers_map).ok();
        let req_payload = payload.clone();

        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO trace_requests \
                 (request_id, tenant_id, project_id, function_id, function_version, method, path, headers, body, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())"
            )
            .bind(&rid)
            .bind(tid)
            .bind(pid)
            .bind(fid)
            .bind("") // function_version will be captured from the runtime deployment info
            .bind(&req_method)
            .bind(&req_path)
            .bind(req_headers)
            .bind(req_payload)
            .execute(&pool)
            .await;
        });
    }

    // 3.5 Schema Validation
    if let Some(schema_val) = &route.json_schema {
        match jsonschema::validator_for(schema_val) {
            Ok(compiled) => {
                if let Err(error) = compiled.validate(&payload) {
                    let err_msgs = vec![error.to_string()];
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "validation_failed",
                            "message": "Request payload failed schema validation",
                            "details": err_msgs
                        }))
                    ).into_response();
                }
            }
            Err(e) => {
                tracing::error!("Failed to compile JSON schema for route {}: {:?}", route.id, e);
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "schema_config_error", "message": "Invalid JSON Schema configured for route" }))
                ).into_response();
            }
        }
    }

    if route.is_async {
        let enqueue_req = CreateJobRequest {
            tenant_id: route.tenant_id,
            project_id: route.project_id,
            function_id: route.function_id,
            payload,
            idempotency_key,
        };

        let mut response = match state.queue_client.enqueue(enqueue_req).await {
            Ok(job) => (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "job_id": job.job_id,
                    "status": "queued"
                })),
            )
                .into_response(),
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "queue_unreachable", "message": e.to_string() })),
            )
                .into_response(),
        };

        if route.cors_enabled {
            let headers = response.headers_mut();

            let origin_str = route.cors_origins
                .as_ref()
                .and_then(|o| if o.is_empty() { None } else { Some(o.join(", ")) })
                .unwrap_or_else(|| "*".to_string());

            let allowed_headers = route.cors_headers
                .as_ref()
                .and_then(|h| if h.is_empty() { None } else { Some(h.join(", ")) })
                .unwrap_or_else(|| "Content-Type, Authorization, X-API-Key".to_string());

            if let Ok(val) = origin_str.parse() {
                headers.insert("Access-Control-Allow-Origin", val);
            }
            headers.insert("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap());
            if let Ok(val) = allowed_headers.parse() {
                headers.insert("Access-Control-Allow-Headers", val);
            }
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

    let forward_payload = serde_json::json!({
        "function_id": route.function_id.to_string(),
        "tenant_id": route.tenant_id.to_string(),
        "project_id": route.project_id.to_string(),
        "payload": payload
    });

    let mut req_builder = state.http_client
        .post(&runtime_url)
        .header("X-Service-Token", &state.internal_service_token)
        .header("X-Tenant-Id", tenant_id.to_string())
        .header("X-Tenant-Slug", &tenant_slug)
        .json(&forward_payload);

    if let Some(uid) = fwd_user_id {
        req_builder = req_builder.header("X-User-Id", uid);
    }
    if let Some(claims) = fwd_jwt_claims {
        req_builder = req_builder.header("X-JWT-Claims", claims);
    }
    // Forward trace IDs to runtime for span hierarchy
    req_builder = req_builder.header("x-request-id", &incoming_request_id);
    if let Some(parent_span) = &incoming_parent_span_id {
        req_builder = req_builder.header("x-parent-span-id", parent_span);
    }

    let runtime_resp = req_builder.send().await;

    // 5. Build Response & Apply CORS
    // Always echo x-request-id back so callers can run `flux trace <id>`.
    let mut response = match runtime_resp {
        Ok(resp) => {
            let status = resp.status();
            let raw = resp.text().await.unwrap_or_default();
            let body: Value = serde_json::from_str(&raw).unwrap_or_else(|_| {
                tracing::warn!(status = %status, raw_body = %&raw[..raw.len().min(200)], "runtime returned non-JSON");
                serde_json::json!({ "error": "runtime_response_parse_error" })
            });
            (status, Json(body)).into_response()
        }
        Err(e) => {
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "runtime_unreachable", "message": e.to_string() }))
            ).into_response()
        }
    };


    // 4.5. Apply CORS
    // Inject trace headers into response so CLI/clients can run `flux trace <id>`.
    if let Ok(val) = incoming_request_id.parse::<HeaderValue>() {
        response.headers_mut().insert("x-request-id", val);
    }
    // If response included x-parent-span-id from runtime, echo it back
    if let Some(parent_span) = &incoming_parent_span_id {
        if let Ok(val) = parent_span.parse::<HeaderValue>() {
            response.headers_mut().insert("x-parent-span-id", val);
        }
    }
    if route.cors_enabled {
        let headers = response.headers_mut();
        
        let origin_str = route.cors_origins
            .as_ref()
            .and_then(|o| if o.is_empty() { None } else { Some(o.join(", ")) })
            .unwrap_or_else(|| "*".to_string());
            
        let allowed_headers = route.cors_headers
            .as_ref()
            .and_then(|h| if h.is_empty() { None } else { Some(h.join(", ")) })
            .unwrap_or_else(|| "Content-Type, Authorization, X-API-Key".to_string());
            
        if let Ok(val) = origin_str.parse() {
            headers.insert("Access-Control-Allow-Origin", val);
        }
        headers.insert("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap());
        if let Ok(val) = allowed_headers.parse() {
            headers.insert("Access-Control-Allow-Headers", val);
        }
    }

    // 6. Asynchronous Analytics
    let status = response.status().as_u16();
    crate::middleware::analytics::log_request(
        &state.metric_tx,
        route.id,
        tenant_id,
        status,
        start_time.elapsed().as_millis() as i64,
    );

    response
}

