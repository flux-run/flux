use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
use crate::engine::pool::IsolatePool;
use crate::engine::wasm_pool::WasmPool;
use crate::secrets::secrets_client::SecretsClient;

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub function_id:     String,
    pub tenant_id:       Uuid,
    pub project_id:      Option<Uuid>,
    pub payload:         Value,
    /// Deterministic randomness seed stored alongside the job in the queue.
    /// When present (replay path), Math.random / crypto.randomUUID / nanoid
    /// are seeded so the same execution_seed produces identical values.
    /// Omit for live invocations — the runtime generates a fresh seed.
    pub execution_seed:  Option<i64>,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub result: Value,
    pub duration_ms: u64,
}

pub struct AppState {
    pub secrets_client: SecretsClient,
    pub http_client: reqwest::Client,
    pub control_plane_url: String,
    pub service_token: String,
    pub bundle_cache: crate::cache::bundle_cache::BundleCache,
    /// Deno V8 isolate pool (JavaScript / TypeScript functions)
    pub isolate_pool: IsolatePool,
    /// Wasmtime execution pool (WASM functions — any language)
    pub wasm_pool: WasmPool,
}

// ── Span helpers ─────────────────────────────────────────────────────────────

/// Compute a short stable fingerprint of the bundle code.
/// Used as `code_sha` in trace spans for replay correlation.
/// Not cryptographic — just sufficient to identify a unique bundle version.
fn bundle_sha(code: &str) -> String {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Extra trace context captured once per request and threaded through all lifecycle spans.
#[derive(Clone)]
struct TraceCtx {
    /// Forwarded from Gateway via `x-parent-span-id` header.
    parent_span_id: Option<String>,
    /// Short fingerprint of the JS bundle being executed — for replay correlation.
    code_sha: Option<String>,
}

/// Fire-and-forget: post a single structured span to the control-plane log sink.
fn post_span(
    client:    &reqwest::Client,
    log_url:   String,
    token:     String,
    fn_id:     String,
    tenant_id: Uuid,
    project_id: Option<Uuid>,
    request_id: Option<String>,
    source:    &'static str,
    level:     &'static str,
    message:   String,
    span_type: &'static str,
) {
    let span_id = Uuid::new_v4().to_string();
    let client = client.clone();
    tokio::spawn(async move {
        let _ = client
            .post(&log_url)
            .header("X-Service-Token", &token)
            .json(&serde_json::json!({
                "source":      source,
                "resource_id": fn_id,
                "tenant_id":   tenant_id,
                "project_id":  project_id,
                "level":       level,
                "message":     message,
                "request_id":  request_id,
                "span_id":     span_id,
                "span_type":   span_type,
            }))
            .send()
            .await;
    });
}

/// Fire-and-forget: post an execution lifecycle span (start / end / error) with
/// full trace context.
///
/// These are the anchors for `flux trace` and `flux why` — they carry:
/// - `parent_span_id`    → links to the Gateway span for end-to-end trace trees
/// - `code_sha`          → fingerprints the exact bundle version for replay
/// - `execution_state`   → "started" | "completed" | "error"
/// - `duration_ms`       → total execution time (only on end/error spans)
fn post_trace_span(
    client:          &reqwest::Client,
    log_url:         String,
    token:           String,
    fn_id:           String,
    tenant_id:       Uuid,
    project_id:      Option<Uuid>,
    request_id:      Option<String>,
    trace_ctx:       &TraceCtx,
    level:           &'static str,
    message:         String,
    span_type:       &'static str,
    execution_state: &'static str,
    duration_ms:     Option<u64>,
) {
    let parent_span_id = trace_ctx.parent_span_id.clone();
    let code_sha       = trace_ctx.code_sha.clone();
    let span_id        = Uuid::new_v4().to_string();
    let client = client.clone();
    tokio::spawn(async move {
        let _ = client
            .post(&log_url)
            .header("X-Service-Token", &token)
            .json(&serde_json::json!({
                "source":           "runtime",
                "resource_id":      fn_id,
                "tenant_id":        tenant_id,
                "project_id":       project_id,
                "level":            level,
                "message":          message,
                "request_id":       request_id,
                "span_id":          span_id,
                "span_type":        span_type,
                "parent_span_id":   parent_span_id,
                "code_sha":         code_sha,
                "execution_state":  execution_state,
                "duration_ms":      duration_ms,
            }))
            .send()
            .await;
    });
}

#[axum::debug_handler]
pub async fn execute_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let tenant_id_header = headers.get("X-Tenant-Id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_else(|| "unknown");
    let tenant_slug_header = headers.get("X-Tenant-Slug")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_else(|| "unknown");
    let request_id = headers.get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    // Forwarded by the Gateway — links this execution's spans to the parent gateway span.
    let parent_span_id = headers.get("x-parent-span-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // TraceCtx is built once per request; code_sha is filled in after the bundle is resolved.
    // We use a mutable binding so we can add code_sha once the bundle code is available.
    let mut trace_ctx = TraceCtx { parent_span_id, code_sha: None };

    // Deterministic replay seed — provided by queue worker for flux queue replay,
    // generated fresh (from UUID entropy) for every live invocation.
    let __seed_bytes = uuid::Uuid::new_v4().into_bytes();
    let execution_seed = req.execution_seed.unwrap_or_else(|| i64::from_le_bytes([
        __seed_bytes[0], __seed_bytes[1], __seed_bytes[2], __seed_bytes[3],
        __seed_bytes[4], __seed_bytes[5], __seed_bytes[6], __seed_bytes[7],
    ]));

    let start_time = std::time::Instant::now();

    // ── WASM warm path: cached bytes → skip control plane + S3 entirely ────────
    if let Some(wasm_bytes) = state.wasm_pool.get_cached_bytes(&req.function_id).await {
        tracing::debug!(function_id = %req.function_id, "wasm bytes cache hit (warm path)");
        let log_url = format!("{}/internal/logs", state.control_plane_url);
        post_span(&state.http_client, log_url.clone(), state.service_token.clone(),
            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
            "runtime", "debug", "wasm cache hit — skipping fetch".into(), "event");
        let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
            Ok(s) => s,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response(),
        };
        post_trace_span(&state.http_client, log_url.clone(), state.service_token.clone(),
            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
            &trace_ctx, "info", "execution_start".into(), "start", "started", None);
        let execution = match state.wasm_pool.execute(
            req.function_id.clone(), wasm_bytes.to_vec(), secrets,
            req.payload, tenant_id_header.to_string(), None,
        ).await {
            Ok(r) => r,
            Err(e) => {
                let (err_code, message) = if let Ok(p) = serde_json::from_str::<serde_json::Value>(&e) {
                    (p.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string(),
                     p.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string())
                } else { ("FunctionExecutionError".to_string(), e) };
                let error_dur = start_time.elapsed().as_millis() as u64;
                post_trace_span(&state.http_client, log_url.clone(), state.service_token.clone(),
                    req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                    &trace_ctx, "error", format!("execution_error: {}: {}", err_code, message),
                    "end", "error", Some(error_dur));
                return (StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": err_code, "message": message })),
                ).into_response();
            }
        };
        let duration_ms = start_time.elapsed().as_millis() as u64;
        {
            let trace_parent  = trace_ctx.parent_span_id.clone();
            let trace_sha     = trace_ctx.code_sha.clone();
            let service_token = state.service_token.clone();
            let function_id   = req.function_id.clone();
            let tenant_id     = req.tenant_id;
            let project_id    = req.project_id;
            let logs          = execution.logs;
            let client        = state.http_client.clone();
            let rid           = request_id.clone();
            let dur           = duration_ms;
            tokio::spawn(async move {
                for log in logs {
                    let span_type = log.span_type.as_deref().unwrap_or("event");
                    let source    = log.source.as_deref().unwrap_or("function");
                    let span_id   = log.span_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                    let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                        .json(&serde_json::json!({
                            "source": source, "resource_id": &function_id,
                            "tenant_id": tenant_id, "project_id": project_id,
                            "level": log.level, "message": log.message,
                            "request_id": &rid, "span_id": span_id, "span_type": span_type,
                            "parent_span_id": &trace_parent, "code_sha": &trace_sha,
                            "duration_ms": log.duration_ms, "tool_name": log.tool_name,
                            "execution_state": log.execution_state,
                        })).send().await;
                }
                let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                    .json(&serde_json::json!({
                        "source": "runtime", "resource_id": &function_id,
                        "tenant_id": tenant_id, "project_id": project_id,
                        "level": "info", "message": format!("execution_end ({}ms)", dur),
                        "request_id": &rid, "span_id": Uuid::new_v4().to_string(),
                        "span_type": "end", "parent_span_id": &trace_parent, "code_sha": &trace_sha,
                        "execution_state": "completed", "duration_ms": dur,
                    })).send().await;
            });
        }
        return (StatusCode::OK, Json(ExecuteResponse { result: execution.output, duration_ms })).into_response();
    }

    // ── Deno function-level bundle cache (skips control plane + S3 entirely) ──
    if let Some(cached_code) = state.bundle_cache.get_by_function(&req.function_id) {
        tracing::debug!(function_id = %req.function_id, "bundle cache hit (function-level)");

        // Fingerprint the bundle for replay correlation.
        trace_ctx.code_sha = Some(bundle_sha(&cached_code));

        // Auto-span: cache hit
        let log_url = format!("{}/internal/logs", state.control_plane_url);
        post_span(&state.http_client, log_url.clone(), state.service_token.clone(),
            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
            "runtime", "debug", "bundle cache hit — skipping fetch".into(), "event");

        let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
            Ok(s) => s,
            Err(e) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response(),
        };
        // execution_start span: anchors the trace tree and records code_sha + parent link.
        post_trace_span(&state.http_client, log_url.clone(), state.service_token.clone(),
            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
            &trace_ctx, "info", "execution_start".into(), "start", "started", None);

        let execution = match state.isolate_pool.execute(
            cached_code,
            secrets,
            req.payload,
            tenant_id_header.to_string(),
            tenant_slug_header.to_string(),
            execution_seed,
        ).await {
            Ok(r) => r,
            Err(e) => {
                let (err_code, message) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&e) {
                    let code = parsed.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string();
                    let msg  = parsed.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string();
                    (code, msg)
                } else {
                    ("FunctionExecutionError".to_string(), e)
                };
                // execution_error span: marks failure with duration for dashboards and flux why.
                let error_dur = start_time.elapsed().as_millis() as u64;
                post_trace_span(&state.http_client, log_url.clone(), state.service_token.clone(),
                    req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                    &trace_ctx, "error", format!("execution_error: {}: {}", err_code, message), "end",
                    "error", Some(error_dur));
                let status = if err_code == "INPUT_VALIDATION_ERROR" { StatusCode::BAD_REQUEST } else { StatusCode::INTERNAL_SERVER_ERROR };
                return (status, Json(serde_json::json!({ "error": err_code, "message": message }))).into_response();
            }
        };
        let duration_ms = start_time.elapsed().as_millis() as u64;
        {
            // Clone trace_ctx fields for move into tokio::spawn
            let trace_parent  = trace_ctx.parent_span_id.clone();
            let trace_sha     = trace_ctx.code_sha.clone();
            let log_url       = format!("{}/internal/logs", state.control_plane_url);
            let service_token = state.service_token.clone();
            let function_id   = req.function_id.clone();
            let tenant_id     = req.tenant_id;
            let project_id    = req.project_id;
            let logs          = execution.logs;
            let client        = state.http_client.clone();
            let rid           = request_id.clone();
            let dur           = duration_ms;
            tokio::spawn(async move {
                for log in logs {
                    let span_type = log.span_type.as_deref().unwrap_or("event");
                    let source    = log.source.as_deref().unwrap_or("function");
                    let span_id   = log.span_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                    let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                        .json(&serde_json::json!({
                            "source":           source,
                            "resource_id":      &function_id,
                            "tenant_id":        tenant_id,
                            "project_id":       project_id,
                            "level":            log.level,
                            "message":          log.message,
                            "request_id":       &rid,
                            "span_id":          span_id,
                            "span_type":        span_type,
                            "parent_span_id":   &trace_parent,
                            "code_sha":         &trace_sha,
                            "duration_ms":      log.duration_ms,
                            "tool_name":        log.tool_name,
                            "execution_state":  log.execution_state,
                        }))
                        .send().await;
                }
                // execution_end span: completes the trace tree entry.
                let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                    .json(&serde_json::json!({
                        "source":           "runtime",
                        "resource_id":      &function_id,
                        "tenant_id":        tenant_id,
                        "project_id":       project_id,
                        "level":            "info",
                        "message":          format!("execution_end ({}ms)", dur),
                        "request_id":       &rid,
                        "span_id":          Uuid::new_v4().to_string(),
                        "span_type":        "end",
                        "parent_span_id":   &trace_parent,
                        "code_sha":         &trace_sha,
                        "execution_state":  "completed",
                        "duration_ms":      dur,
                    }))
                    .send().await;
            });
        }
        return (StatusCode::OK, Json(ExecuteResponse { result: execution.output, duration_ms })).into_response();
    }

    // Fetch secrets from the control plane
    let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response();
        }
    };

    // Auto-span: cache miss
    let log_url_nc = format!("{}/internal/logs", state.control_plane_url);
    post_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
        req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
        "runtime", "debug", "bundle cache miss — fetching from control plane".into(), "event");

    // Fetch real bundle code from the control plane
    let bundle_url = format!(
        "{}/internal/bundle?function_id={}",
        state.control_plane_url, req.function_id
    );

    let bundle_resp = state.http_client
        .get(&bundle_url)
        .header("X-Service-Token", &state.service_token)
        .send()
        .await;

    let code = match bundle_resp {
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "BundleFetchError",
                    "message": format!("Failed to reach control plane: {}", e)
                })),
            ).into_response();
        }
        Ok(resp) => {
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "no_bundle_found",
                        "message": "No active deployment found for this function. Deploy it first."
                    })),
                ).into_response();
            }
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "BundleFetchError",
                        "message": format!("Control plane returned HTTP {}: {}", status, body)
                    })),
                ).into_response();
            }
            let json: serde_json::Value = match resp.json().await {
                Ok(j) => j,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "BundleParseError",
                            "message": format!("Failed to parse bundle response: {}", e)
                        })),
                    ).into_response();
                }
            };
            // Extract runtime field — determines which execution engine to use.
            let bundle_runtime = json.get("data")
                .and_then(|d| d.get("runtime"))
                .and_then(|r| r.as_str())
                .unwrap_or("deno")
                .to_string();

            let (deployment_id, url_opt, code_opt) = {
                let d_id = json.get("data").and_then(|d| d.get("deployment_id")).and_then(|id| id.as_str());
                let u_id = json.get("data").and_then(|d| d.get("url")).and_then(|u| u.as_str());
                let c_id = json.get("data").and_then(|d| d.get("code")).and_then(|c| c.as_str());
                (d_id.map(|s| s.to_string()), u_id.map(|s| s.to_string()), c_id.map(|s| s.to_string()))
            };

            // ── WASM cold execution path ─────────────────────────────────────────
            if bundle_runtime == "wasm" {
                // WASM binaries are downloaded as bytes (not UTF-8 text).
                let wasm_bytes: Vec<u8> = if let Some(url) = url_opt {
                    match state.http_client.get(&url).send().await {
                        Ok(res) if res.status().is_success() => {
                            let fetch_ms = start_time.elapsed().as_millis();
                            post_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
                                req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                                "runtime", "info", format!("wasm bundle fetched from R2 ({}ms)", fetch_ms), "event");
                            res.bytes().await.map(|b| b.to_vec()).unwrap_or_default()
                        }
                        Ok(res) => {
                            let status = res.status().as_u16();
                            let body = res.text().await.unwrap_or_default();
                            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                                "error": "S3FetchError",
                                "message": format!("S3 returned HTTP {}: {}", status, body)
                            }))).into_response();
                        }
                        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                            "error": "S3FetchError",
                            "message": format!("Failed to download wasm bundle: {}", e)
                        }))).into_response(),
                    }
                } else if let Some(encoded) = code_opt {
                    // Inline WASM is base64-encoded (binary can't be stored raw as text).
                    use base64::Engine as _;
                    base64::engine::general_purpose::STANDARD
                        .decode(&encoded)
                        .unwrap_or_else(|_| encoded.into_bytes())
                } else {
                    return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                        "error": "no_bundle_found", "message": "No wasm bundle found for this function."
                    }))).into_response();
                };

                trace_ctx.code_sha = Some(bundle_sha(&String::from_utf8_lossy(&wasm_bytes)));
                state.wasm_pool.cache_bytes(req.function_id.clone(), std::sync::Arc::new(wasm_bytes.clone())).await;

                post_trace_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
                    req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                    &trace_ctx, "info", "execution_start".into(), "start", "started", None);

                let execution = match state.wasm_pool.execute(
                    req.function_id.clone(), wasm_bytes, secrets,
                    req.payload, tenant_id_header.to_string(), None,
                ).await {
                    Ok(r) => r,
                    Err(e) => {
                        let (err_code, message) = if let Ok(p) = serde_json::from_str::<serde_json::Value>(&e) {
                            (p.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string(),
                             p.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string())
                        } else { ("FunctionExecutionError".to_string(), e) };
                        let error_dur = start_time.elapsed().as_millis() as u64;
                        post_trace_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
                            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                            &trace_ctx, "error", format!("execution_error: {}: {}", err_code, message),
                            "end", "error", Some(error_dur));
                        return (StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": err_code, "message": message })),
                        ).into_response();
                    }
                };
                let duration_ms = start_time.elapsed().as_millis() as u64;
                {
                    let trace_parent  = trace_ctx.parent_span_id.clone();
                    let trace_sha     = trace_ctx.code_sha.clone();
                    let log_url       = log_url_nc.clone();
                    let service_token = state.service_token.clone();
                    let function_id   = req.function_id.clone();
                    let tenant_id     = req.tenant_id;
                    let project_id    = req.project_id;
                    let logs          = execution.logs;
                    let client        = state.http_client.clone();
                    let rid           = request_id.clone();
                    let dur           = duration_ms;
                    tokio::spawn(async move {
                        for log in logs {
                            let span_type = log.span_type.as_deref().unwrap_or("event");
                            let source    = log.source.as_deref().unwrap_or("function");
                            let span_id   = log.span_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                            let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                                .json(&serde_json::json!({
                                    "source": source, "resource_id": &function_id,
                                    "tenant_id": tenant_id, "project_id": project_id,
                                    "level": log.level, "message": log.message,
                                    "request_id": &rid, "span_id": span_id, "span_type": span_type,
                                    "parent_span_id": &trace_parent, "code_sha": &trace_sha,
                                    "duration_ms": log.duration_ms, "tool_name": log.tool_name,
                                    "execution_state": log.execution_state,
                                })).send().await;
                        }
                        let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                            .json(&serde_json::json!({
                                "source": "runtime", "resource_id": &function_id,
                                "tenant_id": tenant_id, "project_id": project_id,
                                "level": "info", "message": format!("execution_end ({}ms)", dur),
                                "request_id": &rid, "span_id": Uuid::new_v4().to_string(),
                                "span_type": "end", "parent_span_id": &trace_parent, "code_sha": &trace_sha,
                                "execution_state": "completed", "duration_ms": dur,
                            })).send().await;
                    });
                }
                return (StatusCode::OK, Json(ExecuteResponse { result: execution.output, duration_ms })).into_response();
            }

            // ── Deno cold path ──────────────────────────────────────────────────
            let final_code = if let Some(d_id) = deployment_id.clone() {
                if let Some(cached_code) = state.bundle_cache.get(&d_id) {
                    tracing::debug!(function_id = %req.function_id, deployment_id = %d_id, "bundle cache hit (deployment-level) — re-warming function cache");
                    // Re-warm the function-level cache so the next call skips the control plane.
                    state.bundle_cache.insert_both(req.function_id.clone(), Some(d_id), cached_code.clone());
                    Some(cached_code)
                } else { None }
            } else { None };

            if let Some(c) = final_code {
                c
            } else if let Some(url) = url_opt {
                let s3_resp = state.http_client.get(&url).send().await;
                match s3_resp {
                    Ok(res) if res.status().is_success() => {
                        let text = res.text().await.unwrap_or_default();
                        let fetch_ms = start_time.elapsed().as_millis();
                        tracing::debug!(function_id = %req.function_id, deployment_id = ?deployment_id, "bundle cache miss — caching");
                        state.bundle_cache.insert_both(req.function_id.clone(), deployment_id, text.clone());
                        post_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
                            req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                            "runtime", "info", format!("bundle fetched from R2 ({}ms)", fetch_ms), "event");
                        text
                    }
                    Ok(res) => {
                        let status = res.status().as_u16();
                        let body = res.text().await.unwrap_or_default();
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "error": "S3FetchError",
                                "message": format!("S3 returned HTTP {}: {}", status, body)
                            })),
                        ).into_response();
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "error": "S3FetchError",
                                "message": format!("Failed to download bundle from R2/S3 presigned URL: {}", e)
                            })),
                        ).into_response();
                    }
                }
            } else if let Some(code_str) = code_opt {
                // Fallback to inline database storage
                tracing::debug!(function_id = %req.function_id, "bundle cache miss (inline) — caching");
                state.bundle_cache.insert_both(req.function_id.clone(), deployment_id, code_str.clone());
                code_str
            } else {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "no_bundle_found",
                        "message": "Bundle response did not contain url or code field"
                    })),
                ).into_response();
            }
        }
    };

    // Fingerprint the bundle for replay correlation.
    trace_ctx.code_sha = Some(bundle_sha(&code));

    // execution_start span: anchors the trace tree and records code_sha + parent link.
    post_trace_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
        req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
        &trace_ctx, "info", "execution_start".into(), "start", "started", None);

    // Execute the function with the new framework-aware executor
    let execution = match state.isolate_pool.execute(
        code,
        secrets,
        req.payload,
        tenant_id_header.to_string(),
        tenant_slug_header.to_string(),
        execution_seed,
    ).await {
        Ok(r) => r,
        Err(e) => {
            // Parse structured error from the framework if available
            let (err_code, message) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&e) {
                let code = parsed.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string();
                let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string();
                (code, msg)
            } else {
                ("FunctionExecutionError".to_string(), e)
            };

            let status = if err_code == "INPUT_VALIDATION_ERROR" {
                StatusCode::BAD_REQUEST
            } else if err_code == "OUTPUT_VALIDATION_ERROR" {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            // execution_error span: marks failure with duration for dashboards and flux why.
            let error_dur = start_time.elapsed().as_millis() as u64;
            post_trace_span(&state.http_client, log_url_nc.clone(), state.service_token.clone(),
                req.function_id.clone(), req.tenant_id, req.project_id, request_id.clone(),
                &trace_ctx, "error", format!("execution_error: {}: {}", err_code, message), "end",
                "error", Some(error_dur));

            return (status, Json(serde_json::json!({ "error": err_code, "message": message }))).into_response();
        }
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;

    // Fire-and-forget: forward ctx.log() lines to /internal/logs
    {
        // Clone trace_ctx fields for move into tokio::spawn
        let trace_parent   = trace_ctx.parent_span_id.clone();
        let trace_sha      = trace_ctx.code_sha.clone();
        let log_url        = format!("{}/internal/logs", state.control_plane_url);
        let service_token  = state.service_token.clone();
        let function_id    = req.function_id.clone();
        let tenant_id      = req.tenant_id;
        let project_id     = req.project_id;
        let logs           = execution.logs;
        let client         = state.http_client.clone();
        let rid            = request_id.clone();
        let dur            = duration_ms;

        tokio::spawn(async move {
            for log in logs {
                let span_type = log.span_type.as_deref().unwrap_or("event");
                let source    = log.source.as_deref().unwrap_or("function");
                let span_id   = log.span_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                let _ = client
                    .post(&log_url)
                    .header("X-Service-Token", &service_token)
                    .json(&serde_json::json!({
                        "source":           source,
                        "resource_id":      &function_id,
                        "tenant_id":        tenant_id,
                        "project_id":       project_id,
                        "level":            log.level,
                        "message":          log.message,
                        "request_id":       &rid,
                        "span_id":          span_id,
                        "span_type":        span_type,
                        "parent_span_id":   &trace_parent,
                        "code_sha":         &trace_sha,
                        "duration_ms":      log.duration_ms,
                        "tool_name":        log.tool_name,
                        "execution_state":  log.execution_state,
                    }))
                    .send()
                    .await;
            }
            // execution_end span: completes the trace tree entry.
            let _ = client
                .post(&log_url)
                .header("X-Service-Token", &service_token)
                .json(&serde_json::json!({
                    "source":           "runtime",
                    "resource_id":      &function_id,
                    "tenant_id":        tenant_id,
                    "project_id":       project_id,
                    "level":            "info",
                    "message":          format!("execution_end ({}ms)", dur),
                    "request_id":       &rid,
                    "span_id":          Uuid::new_v4().to_string(),
                    "span_type":        "end",
                    "parent_span_id":   &trace_parent,
                    "code_sha":         &trace_sha,
                    "execution_state":  "completed",
                    "duration_ms":      dur,
                }))
                .send()
                .await;
        });
    }

    (
        StatusCode::OK,
        Json(ExecuteResponse { result: execution.output, duration_ms }),
    ).into_response()
}

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /internal/cache/invalidate
///
/// Called by the control plane immediately after a new deployment goes live,
/// so the runtime stops serving the old bundle within milliseconds instead of
/// waiting for the 60-second function-cache TTL to expire.
///
/// Body (all fields optional – omit any you don't want to invalidate):
/// ```json
/// { "function_id": "...", "deployment_id": "...",
///   "tenant_id":   "...", "project_id":    "..." }
/// ```
#[derive(Deserialize)]
pub struct InvalidateCacheRequest {
    pub function_id:   Option<String>,
    pub deployment_id: Option<String>,
    pub tenant_id:     Option<uuid::Uuid>,
    pub project_id:    Option<uuid::Uuid>,
}

pub async fn invalidate_cache_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<InvalidateCacheRequest>,
) -> impl IntoResponse {
    // Require the service token so this endpoint is not publicly callable.
    let provided = headers
        .get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if provided != state.service_token {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    let mut evicted: Vec<&str> = Vec::new();

    if let Some(ref fid) = req.function_id {
        state.bundle_cache.invalidate_function(fid);
        state.wasm_pool.evict(fid).await;
        evicted.push("function_bundle");
    }
    if let Some(ref did) = req.deployment_id {
        state.bundle_cache.invalidate_deployment(did);
        evicted.push("deployment_bundle");
    }
    if let Some(tid) = req.tenant_id {
        state.secrets_client.cache().invalidate(tid, req.project_id);
        evicted.push("secrets");
    }

    tracing::info!(
        function_id   = ?req.function_id,
        deployment_id = ?req.deployment_id,
        tenant_id     = ?req.tenant_id,
        "cache invalidated: {:?}", evicted
    );

    (StatusCode::OK, Json(serde_json::json!({ "evicted": evicted }))).into_response()
}
