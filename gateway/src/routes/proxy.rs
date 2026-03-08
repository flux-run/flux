use axum::{
    extract::{Path, State},
    http::{Method, Request},
    response::{IntoResponse, Response},
    Json,
};
use axum::body::Body;
use crate::state::SharedState;
use crate::services::route_lookup::lookup_route;
use serde_json::Value;

pub async fn proxy_handler(
    State(state): State<SharedState>,
    method: Method,
    Path(path): Path<String>,
    req: Request<Body>,
) -> Response {
    let full_path = format!("/{}", path);
    let method_str = method.to_string();

    // 0. Extract Resolved Identity
    let identity = match req.extensions().get::<crate::middleware::identity_resolver::ResolvedIdentity>() {
        Some(id) => id,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "identity_resolution_failed", "message": "Could not resolve project/tenant identity from host" }))
            ).into_response();
        }
    };

    // 1. Resolve Route
    let route = match lookup_route(&state.db_pool, identity.tenant_id, &full_path, &method_str).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "route_not_found", "message": format!("No route for {} {}", method_str, full_path) }))
            ).into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "internal_error", "message": e.to_string() }))
            ).into_response();
        }
    };

    // 2. Authenticate
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

    let forward_payload = serde_json::json!({
        "function_id": route.function_id.to_string(),
        "tenant_id": route.tenant_id.to_string(),
        "project_id": route.project_id.to_string(),
        "payload": payload
    });

    let runtime_resp = state.http_client
        .post(&runtime_url)
        .header("X-Service-Token", &state.internal_service_token)
        .json(&forward_payload)
        .send()
        .await;

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


    if route.cors_enabled {
        let headers = response.headers_mut();
        headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
        headers.insert("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap());
        headers.insert("Access-Control-Allow-Headers", "Content-Type, X-API-Key".parse().unwrap());
    }

    response
}

