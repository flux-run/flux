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
    extract_filter_cols, extract_role_from_jwt, extract_table_hint, is_query_cacheable,
    CacheEntry, QueryCacheKey,
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

// ── DB query span helper ──────────────────────────────────────────────────

/// Slow-query threshold — DB queries taking longer than this are logged at WARN.
const SLOW_DB_MS: u64 = 50;

/// Fire-and-forget: insert a `source=db` span into platform_logs so the query
/// appears in distributed traces.  Runs in a detached tokio task to keep the
/// hot proxy path free of extra latency.
fn post_db_span(
    pool: sqlx::PgPool,
    tenant_id: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
    request_id: String,
    table: String,
    duration_ms: u64,
    cache: &'static str,
    filter_cols: Vec<String>,
) {
    let is_slow = duration_ms >= SLOW_DB_MS;
    let level   = if is_slow { "warn" } else { "info" };
    let message = match cache {
        "hit" => format!("db query on {} (edge cache)", table),
        _ if is_slow => format!("slow db query on {} ({}ms)", table, duration_ms),
        _ => format!("db query on {} ({}ms)", table, duration_ms),
    };
    let metadata = serde_json::json!({
        "table":        table,
        "duration_ms":  duration_ms,
        "cache":        cache,
        "slow":         is_slow,
        "filter_cols":  filter_cols,
    });
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO platform_logs \
             (tenant_id, project_id, source, resource_id, level, message, request_id, span_type, metadata) \
             VALUES ($1, $2, 'db', $3, $4, $5, $6, 'event', $7)",
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(&table)
        .bind(level)
        .bind(&message)
        .bind(&request_id)
        .bind(metadata)
        .execute(&pool)
        .await;
    });
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

    // For db query spans — resolve owning tenant/project from caller headers.
    let span_tenant_id: Option<uuid::Uuid> = in_headers
        .get("x-fluxbase-tenant")
        .or_else(|| in_headers.get("x-tenant-id"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    let span_project_id: Option<uuid::Uuid> = in_headers
        .get("x-fluxbase-project")
        .or_else(|| in_headers.get("x-project-id"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    // Table hint and filter columns extracted once for all /db/query spans.
    let (table_hint, filter_cols): (String, Vec<String>) = if uri.path().ends_with("/db/query") {
        (
            extract_table_hint(&body_bytes).unwrap_or_else(|| "unknown".to_string()),
            extract_filter_cols(&body_bytes),
        )
    } else {
        (String::new(), vec![])
    };
    let query_start = Instant::now();

    // ── Improvement #1: Per-tenant rate limiting ───────────────────────────────
    // Token bucket fills at rate_limit_per_sec tokens/second; burst = limit.
    // Keyed by tenant_id to isolate tenants from each other.
    let rate_key = span_tenant_id
        .map(|t| t.to_string())
        .or_else(|| {
            in_headers
                .get("x-fluxbase-tenant")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "anonymous".to_string());

    if !crate::middleware::rate_limit::check_rate_limit(&rate_key, state.rate_limit_per_sec) {
        tracing::warn!(tenant = %rate_key, "gateway: rate limit exceeded");
        let body = serde_json::json!({
            "error": "rate_limited",
            "message": "Too many requests. Please slow down and retry."
        });
        let mut resp = (axum::http::StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
        resp.headers_mut().extend(cors_headers());
        return Ok(resp);
    }

    // ── Improvement #2: Query structural validation ──────────────────────────
    // Cheap O(columns) check applied before forwarding to the data-engine.
    // Rejects: too many columns, too-deeply-nested selectors, too many filters.
    // Malformed JSON is passed through so the data-engine returns a consistent error.
    if uri.path().ends_with("/db/query") && !body_bytes.is_empty() {
        if let Err((status, msg)) = crate::middleware::query_guard::validate_query_body(
            &body_bytes,
            &state.query_guard_config,
        ) {
            tracing::warn!(tenant = %rate_key, %msg, "gateway: query too complex");
            let body = serde_json::json!({
                "error": "query_too_complex",
                "message": msg
            });
            let mut resp = (status, axum::Json(body)).into_response();
            resp.headers_mut().extend(cors_headers());
            return Ok(resp);
        }
    }

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
                // Span: edge cache hit — query was NOT forwarded to the DB.
                if !table_hint.is_empty() {
                    if let Some(tid) = span_tenant_id {
                        post_db_span(
                            state.db_pool.clone(), tid, span_project_id,
                            request_id.clone(), table_hint.clone(),
                            query_start.elapsed().as_millis() as u64, "hit",
                            filter_cols.clone(),
                        );
                    }
                }
                return Ok(resp);
            }

            // ── Improvement #3: Tenant concurrency budget ───────────────────
            // Cache HITs bypass this entirely (no DB touch).
            // try_acquire_owned() never queues — returns 429 when at capacity.
            let _tenant_permit = {
                let sem = state
                    .tenant_semaphores
                    .entry(rate_key.clone())
                    .or_insert_with(|| {
                        std::sync::Arc::new(tokio::sync::Semaphore::new(
                            state.max_concurrent_per_tenant,
                        ))
                    })
                    .clone();
                match sem.try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::warn!(tenant = %rate_key, "gateway: concurrency limit exceeded");
                        let body = serde_json::json!({
                            "error": "concurrency_limit",
                            "message": "Too many concurrent requests for this tenant. Retry shortly."
                        });
                        let mut resp = (axum::http::StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
                        resp.headers_mut().extend(cors_headers());
                        return Ok(resp);
                    }
                }
            };

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
                    // Span: actual DB query (cache miss).
                    if !table_hint.is_empty() {
                        if let Some(tid) = span_tenant_id {
                            post_db_span(
                                state.db_pool.clone(), tid, span_project_id,
                                request_id.clone(), table_hint.clone(),
                                query_start.elapsed().as_millis() as u64, "miss",
                                filter_cols.clone(),
                            );
                        }
                    }
                    Ok(resp)
                }
                Err(()) => Err(StatusCode::BAD_GATEWAY),
            };
        }
    }

    // ── Non-cacheable path (writes, file ops, missing project header) ─────
    // ── Improvement #3: Tenant concurrency budget (non-cacheable path) ────
    let _tenant_permit_nc = {
        let sem = state
            .tenant_semaphores
            .entry(rate_key.clone())
            .or_insert_with(|| {
                std::sync::Arc::new(tokio::sync::Semaphore::new(
                    state.max_concurrent_per_tenant,
                ))
            })
            .clone();
        match sem.try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!(tenant = %rate_key, "gateway: concurrency limit exceeded (non-cacheable)");
                let body = serde_json::json!({
                    "error": "concurrency_limit",
                    "message": "Too many concurrent requests for this tenant. Retry shortly."
                });
                let mut resp = (axum::http::StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
                resp.headers_mut().extend(cors_headers());
                return Ok(resp);
            }
        }
    };
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
    // Span: non-cacheable db query (write, file op, or large result).
    if !table_hint.is_empty() {
        if let Some(tid) = span_tenant_id {
            post_db_span(
                state.db_pool.clone(), tid, span_project_id,
                request_id.clone(), table_hint.clone(),
                query_start.elapsed().as_millis() as u64, "bypass",
                filter_cols.clone(),
            );
        }
    }
    Ok(response)
}
