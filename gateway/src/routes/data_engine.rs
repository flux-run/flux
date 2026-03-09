/// Transparent reverse-proxy for Data-Engine execution routes, with
/// permission-aware edge caching and single-flight concurrency protection.
///
/// Pipeline for POST /db/query:
///   1. CORS preflight fast-path
///   2. is_query_cacheable()  — skip offset / large limit / random-order
///   3. cache HIT             — return stored bytes, X-Cache: HIT
///   4. inflight HIT          — coalesce onto existing backend call (single-flight)
///   5. inflight MISS         — execute backend call, populate cache, X-Cache: MISS
///
/// All other paths bypass the cache entirely (X-Cache: BYPASS).
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::FutureExt;
use std::time::Instant;

use crate::cache::query_cache::{
    extract_role_from_jwt, extract_table_hint, is_query_cacheable, CacheEntry, QueryCacheKey,
};
use crate::state::SharedState;

// ── CORS ──────────────────────────────────────────────────────────────────

const CORS_ORIGIN:  &str = "*";
const CORS_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const CORS_HEADERS: &str =
    "Authorization, Content-Type, Accept, X-Fluxbase-Tenant, X-Fluxbase-Project";

fn cors_headers() -> HeaderMap {
    let mut m = HeaderMap::new();
    m.insert("access-control-allow-origin",  HeaderValue::from_static(CORS_ORIGIN));
    m.insert("access-control-allow-methods", HeaderValue::from_static(CORS_METHODS));
    m.insert("access-control-allow-headers", HeaderValue::from_static(CORS_HEADERS));
    m
}

// ── Cacheability gate ─────────────────────────────────────────────────────

/// Only `POST …/db/query` is eligible for caching.
fn is_cacheable(method: &axum::http::Method, path: &str) -> bool {
    method == axum::http::Method::POST && path.ends_with("/db/query")
}

// ── Backend proxy helper ──────────────────────────────────────────────────

/// Executes a single HTTP request against the data-engine and returns
/// `(status_u16, content_type, body_bytes)`.
///
/// All parameters are **owned** so this function can be moved into a
/// `'static` future and shared across concurrent waiters via [`QueryCache::get_or_fetch`].
/// `forward_headers` is a list of `(header-name, header-value)` pairs to forward.
async fn do_proxy(
    client: reqwest::Client,
    method: reqwest::Method,
    target_url: String,
    forward_headers: Vec<(String, String)>,
    service_token: String,
    request_id: String,
    body: bytes::Bytes,
) -> Result<(u16, String, bytes::Bytes), ()> {
    let mut builder = client.request(method, &target_url);

    for (name, value) in &forward_headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder = builder
        .header("x-service-token", &service_token)
        .header("x-request-id", &request_id);

    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }

    let upstream = builder.send().await.map_err(|e| {
        tracing::error!("data-engine proxy error: {:?}", e);
    })?;

    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let resp_bytes = upstream
        .bytes()
        .await
        .map(|b| bytes::Bytes::from(b.to_vec()))
        .map_err(|_| ())?;

    Ok((status, content_type, resp_bytes))
}

// ── Public handler ────────────────────────────────────────────────────────

pub async fn proxy_handler(
    State(state): State<SharedState>,
    req: Request,
) -> Result<Response, StatusCode> {
    let method     = req.method().clone();
    let uri        = req.uri().clone();
    let in_headers = req.headers().clone();

    // ── CORS preflight ────────────────────────────────────────────────────
    if method == axum::http::Method::OPTIONS {
        let mut resp = (StatusCode::NO_CONTENT, "").into_response();
        resp.headers_mut().extend(cors_headers());
        return Ok(resp);
    }

    // ── Collect common values ─────────────────────────────────────────────
    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or_else(|| uri.path());
    let target_url = format!("{}{}", state.data_engine_url, path_and_query);

    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let request_id = in_headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Headers to forward to the data-engine.
    let forward_headers: Vec<(String, String)> = in_headers
        .iter()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "authorization"
                    | "content-type"
                    | "accept"
                    | "x-fluxbase-tenant"
                    | "x-fluxbase-project"
            )
        })
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.to_string(), v.to_string())))
        .collect();

    // ── Cache-eligible path ───────────────────────────────────────────────
    let cacheable = is_cacheable(&method, uri.path());

    if cacheable && is_query_cacheable(&body_bytes) {
        let project_id = in_headers
            .get("x-fluxbase-project")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let role = extract_role_from_jwt(
            in_headers.get("authorization").and_then(|v| v.to_str().ok()),
        );

        if !project_id.is_empty() {
            let cache_key = QueryCacheKey::new(project_id, &role, &body_bytes);

            // ── Cache HIT — return immediately ────────────────────────────
            if let Some(entry) = state.query_cache.get(&cache_key) {
                tracing::debug!(project_id, age_ms = entry.age_ms(), "query cache HIT");
                let mut resp = axum::response::Response::builder()
                    .status(entry.status)
                    .header("content-type", &entry.content_type)
                    .header("x-cache", "HIT")
                    .header("x-cache-age", entry.age_ms().to_string())
                    .header("x-request-id", &request_id)
                    .body(axum::body::Body::from(entry.body))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                resp.headers_mut().extend(cors_headers());
                return Ok(resp);
            }

            // ── Single-flight MISS — coalesce concurrent requests ─────────
            // Capture everything the future needs as owned values so it is 'static.
            let client        = state.http_client.clone();
            let req_method    = reqwest::Method::from_bytes(method.as_str().as_bytes())
                                    .map_err(|_| StatusCode::BAD_REQUEST)?;
            let url           = target_url.clone();
            let fwd_hdrs      = forward_headers.clone();
            let svc_token     = state.internal_service_token.clone();
            let req_id        = request_id.clone();
            let body_owned    = body_bytes.clone();
            let body_for_hint = body_bytes.clone();
            let ttl           = state.query_cache.ttl;

            let result = state
                .query_cache
                .get_or_fetch(cache_key, move || {
                    async move {
                        let (status, content_type, resp_bytes) =
                            do_proxy(client, req_method, url, fwd_hdrs, svc_token, req_id, body_owned)
                                .await?;

                        // Only cache successful responses.
                        if !StatusCode::from_u16(status)
                            .map(|s| s.is_success())
                            .unwrap_or(false)
                        {
                            return Err(());
                        }

                        let table_hint = extract_table_hint(&body_for_hint);
                        Ok(CacheEntry {
                            body: resp_bytes,
                            status,
                            content_type,
                            table_hint,
                            cached_at: Instant::now(),
                            ttl,
                        })
                    }
                    .boxed()
                })
                .await;

            return match result {
                Ok(entry) => {
                    tracing::debug!(project_id, "query cache MISS → stored");
                    let mut resp = axum::response::Response::builder()
                        .status(entry.status)
                        .header("content-type", &entry.content_type)
                        .header("x-cache", "MISS")
                        .header("x-request-id", &request_id)
                        .body(axum::body::Body::from(entry.body))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    resp.headers_mut().extend(cors_headers());
                    Ok(resp)
                }
                Err(()) => Err(StatusCode::BAD_GATEWAY),
            };
        }
    }

    // ── Non-cacheable path (writes, file ops, missing project header) ─────
    let req_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let (status_u16, content_type, resp_bytes) = do_proxy(
        state.http_client.clone(),
        req_method,
        target_url,
        forward_headers,
        state.internal_service_token.clone(),
        request_id.clone(),
        body_bytes,
    )
    .await
    .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = StatusCode::from_u16(status_u16).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = axum::response::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("x-request-id", &request_id)
        .header("x-cache", "BYPASS")
        .body(axum::body::Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    response.headers_mut().extend(cors_headers());
    Ok(response)
}
