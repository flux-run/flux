use axum::{
    extract::{Path, State},
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum::body::Body;
use crate::state::SharedState;
use job_contract::job::CreateJobRequest;
use serde_json::Value;

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

    // 1. Resolve Route from memory snapshot
    let snapshot_data = state.snapshot.get_data().await;
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
        if !crate::middleware::rate_limit::check_rate_limit(&limit_key, limit) {
            return (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "rate_limit_exceeded", "message": "Too many requests. Please try again later." }))
            ).into_response();
        }
    }

    // 4. Forward to Runtime
    let runtime_url = format!("{}/execute", state.runtime_url);
    
    // We need to extract the payload from the original request.
    // For now, we assume it's JSON.
    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await { // 10MB limit
        Ok(b) => b,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "payload_too_large", "message": e.to_string() }))
            ).into_response();
        }
    };

    let payload: Value = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));

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
            state.db_pool.clone(),
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

    let runtime_resp = req_builder.send().await;

    // 5. Build Response & Apply CORS
    let mut response = match runtime_resp {
        Ok(resp) => {
            let status = resp.status();
            let body: Value = resp.json().await.unwrap_or(serde_json::json!({ "error": "runtime_response_parse_error" }));
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
        state.db_pool.clone(),
        route.id,
        tenant_id,
        status,
        start_time.elapsed().as_millis() as i64,
    );

    response
}

