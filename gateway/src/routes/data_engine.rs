/// Transparent reverse-proxy for Data-Engine execution routes, with
/// permission-aware edge caching, single-flight concurrency protection,
/// and zero-copy response reuse via `Arc<HeaderMap>` + `Bytes`.
///
/// Pipeline for POST /db/query:
///   1. CORS preflight fast-path
///   2. is_query_cacheable()  — skip offset / large limit / random-order
///   3. cache HIT             — Arc clone headers + O(1) Bytes clone, X-Cache: HIT
///   4. inflight HIT          — coalesce onto existing backend call (single-flight)
///   5. inflight MISS         — execute backend call, strip + store headers, X-Cache: MISS
///
/// All other paths bypass the cache entirely (X-Cache: BYPASS).
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::FutureExt;
use std::sync::Arc;
use std::time::Instant;

use crate::cache::query_cache::{
    extract_role_from_jwt, extract_table_hint, is_query_cacheable, CacheEntry, QueryCacheKey,
};
use crate::state::SharedState;

// ── CORS ──────────────────────────────────────────────────────────────────

const CORS_ORIGIN:  &str = "*";
const CORS_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const CORS_HEADERS: &str =
    "Authorization, Content-Type, Accept, X-Fluxbase-Tenant, X-Fluxbase-Project, X-Tenant-Id, X-Project-Id, X-Tenant-Slug, X-Project-Slug, X-User-Id, X-User-Role";

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

/// Executes a single HTTP request against the data-engine.
///
/// Returns `(status, response_headers, body_bytes)` where `response_headers`
/// already has sensitive / per-request headers stripped so the result can be
/// stored directly in the edge cache without further processing.
/// `forward_headers` is a list of `(header-name, header-value)` pairs to forward.
async fn do_proxy(
    client: reqwest::Client,
    method: reqwest::Method,
    target_url: String,
    forward_headers: Vec<(String, String)>,
    service_token: String,
    request_id: String,
    body: bytes::Bytes,
) -> Result<(StatusCode, Arc<HeaderMap>, bytes::Bytes), ()> {
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

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    // Collect upstream headers, then strip anything that must not be cached or
    // forwarded unchanged (per-request IDs, session cookies, framing headers).
    let mut resp_headers = HeaderMap::new();
    for (name, value) in upstream.headers() {
        resp_headers.insert(name, value.clone());
    }
    CacheEntry::strip_sensitive(&mut resp_headers);

    let resp_bytes = upstream
        .bytes()
        .await
        .map(|b| bytes::Bytes::from(b.to_vec()))
        .map_err(|_| ())?;

    Ok((status, Arc::new(resp_headers), resp_bytes))
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
                    | "x-tenant-id"
                    | "x-project-id"
                    | "x-tenant-slug"
                    | "x-project-slug"
                    | "x-user-id"
                    | "x-user-role"
            )
        })
        .filter_map(|(n, v)| v.to_str().ok().map(|v| (n.to_string(), v.to_string())))
        .collect();

    // ── Cache-eligible path ───────────────────────────────────────────────
    //
    // is_query_cacheable() contains a serde_json parse to inspect offset/limit/order.
    // We intentionally defer it to the MISS storage gate (inside get_or_fetch)
    // so that cache HITs pay zero JSON-parsing cost:
    //
    //   HIT  path: is_cacheable (O(1)) → hash (O(min(n,4K))) → DashMap lookup → return
    //   MISS path: proxy → is_query_cacheable → maybe store → return
    let cacheable = is_cacheable(&method, uri.path());

    if cacheable {
        let project_id = in_headers
            .get("x-fluxbase-project")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let role = extract_role_from_jwt(
            in_headers.get("authorization").and_then(|v| v.to_str().ok()),
        );

        if !project_id.is_empty() {
            let cache_key = QueryCacheKey::new(project_id, &role, &body_bytes);

            // ── Cache HIT — zero-copy return ──────────────────────────────
            if let Some(entry) = state.query_cache.get(&cache_key) {
                let age = entry.age_ms();
                tracing::debug!(project_id, age_ms = age, "query cache HIT");

                let mut builder = axum::response::Response::builder()
                    .status(entry.status)
                    .header("x-cache", "HIT")
                    .header("x-cache-age", age.to_string())
                    .header("x-request-id", &request_id);

                // Arc clone — no HeaderMap allocation, no per-header inserts.
                for (k, v) in entry.headers.iter() {
                    builder = builder.header(k, v);
                }

                // Bytes::clone() is O(1) — it is Arc<[u8]> + offset + len.
                let mut resp = builder
                    .body(axum::body::Body::from(entry.body.clone()))
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
                        let (status, headers, resp_bytes) =
                            do_proxy(client, req_method, url, fwd_hdrs, svc_token, req_id, body_owned)
                                .await?;

                        // Only cache successful responses.
                        if !status.is_success() {
                            return Err(());
                        }

                        // JSON parse happens here — only on a true backend MISS,
                        // never on a HIT. Filters out offset / large-limit / random-order.
                        if !is_query_cacheable(&body_for_hint) {
                            return Err(());
                        }

                        let table_hint = extract_table_hint(&body_for_hint);
                        Ok(CacheEntry {
                            body: resp_bytes,
                            headers,
                            status,
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

                    let mut builder = axum::response::Response::builder()
                        .status(entry.status)
                        .header("x-cache", "MISS")
                        .header("x-request-id", &request_id);

                    // Same zero-copy path as HIT.
                    for (k, v) in entry.headers.iter() {
                        builder = builder.header(k, v);
                    }

                    let mut resp = builder
                        .body(axum::body::Body::from(entry.body.clone()))
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

    let (status, resp_headers, resp_bytes) = do_proxy(
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

    let mut builder = axum::response::Response::builder()
        .status(status)
        .header("x-request-id", &request_id)
        .header("x-cache", "BYPASS");

    for (k, v) in resp_headers.iter() {
        builder = builder.header(k, v);
    }

    let mut response = builder
        .body(axum::body::Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    response.headers_mut().extend(cors_headers());
    Ok(response)
}
