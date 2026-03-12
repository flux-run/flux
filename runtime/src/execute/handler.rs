/// execute_handler — HTTP handler for `POST /execute`.
///
/// Single responsibility: parse the request into an `InvocationCtx`, delegate
/// to `BundleResolver` for bundle resolution and `ExecutionRunner` for execution.
/// All business logic lives in those modules; this file stays under 100 lines.
use std::sync::Arc;
use std::time::Instant;
use axum::{
    extract::State,
    http::HeaderMap,
    Json,
    response::IntoResponse,
};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::AppState;
use crate::execute::bundle::{BundleResolver, ResolvedBundle, bundle_sha};
use crate::execute::runner::{ExecutionRunner, allowed_wasm_http_hosts};
use crate::execute::types::{ExecuteRequest, InvocationCtx};
use crate::trace::emitter::TraceEmitter;

#[axum::debug_handler]
pub async fn execute_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    // ── Build per-request context ─────────────────────────────────────────
    let request_id = headers.get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let parent_span_id = headers.get("x-parent-span-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let runtime_hint = headers.get("X-Function-Runtime")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Deterministic replay seed — provided by queue for replay, fresh UUID for live.
    let seed_bytes = Uuid::new_v4().into_bytes();
    let execution_seed = req.execution_seed.unwrap_or_else(|| i64::from_le_bytes([
        seed_bytes[0], seed_bytes[1], seed_bytes[2], seed_bytes[3],
        seed_bytes[4], seed_bytes[5], seed_bytes[6], seed_bytes[7],
    ]));

    let ctx = InvocationCtx {
        function_id:    req.function_id.clone(),
        project_id:     req.project_id,
        payload:        req.payload,
        execution_seed,
        request_id:     request_id.clone(),
        parent_span_id: parent_span_id.clone(),
    };

    let log_url = format!("{}/internal/logs", state.api_url);
    let mut tracer = TraceEmitter::new(
        state.http_client.clone(),
        log_url,
        state.service_token.clone(),
        ctx.function_id.clone(),
        ctx.project_id,
        ctx.request_id.clone(),
        ctx.parent_span_id.clone(),
    );

    let start = Instant::now();
    let resolver = BundleResolver {
        bundle_cache:  &state.bundle_cache,
        schema_cache:  &state.schema_cache,
        wasm_pool:     &state.wasm_pool,
        http_client:   &state.http_client,
        api_url:       &state.api_url,
        service_token: &state.service_token,
    };
    let runner = ExecutionRunner {
        isolate_pool:    &state.isolate_pool,
        wasm_pool:       &state.wasm_pool,
        schema_cache:    &state.schema_cache,
        http_client:     &state.http_client,
        wasm_http_hosts: allowed_wasm_http_hosts(),
    };

    // ── Warm WASM path ────────────────────────────────────────────────────
    if runtime_hint != "deno" {
        if let Some(wasm_bytes) = resolver.warm_wasm(&ctx.function_id).await {
            tracer.post_event("debug", "wasm bytes cache hit — skipping fetch".into());
            tracer.code_sha = Some(bundle_sha(&wasm_bytes));
            let secrets = match fetch_secrets(&state, ctx.project_id).await { Ok(s) => s, Err(r) => return r };
            return runner.run(ResolvedBundle::Wasm { bytes: wasm_bytes.to_vec() }, secrets, &ctx, &tracer, start).await;
        }
    }

    // ── Warm Deno path ────────────────────────────────────────────────────
    if runtime_hint != "wasm" {
        if let Some(cached_code) = resolver.warm_deno(&ctx.function_id) {
            tracer.post_event("debug", "bundle cache hit — skipping fetch".into());
            tracer.code_sha = Some(bundle_sha(cached_code.as_bytes()));
            let secrets = match fetch_secrets(&state, ctx.project_id).await { Ok(s) => s, Err(r) => return r };
            return runner.run(ResolvedBundle::Deno { code: cached_code }, secrets, &ctx, &tracer, start).await;
        }
    }

    // ── Cold path ─────────────────────────────────────────────────────────
    tracer.post_event("debug", "bundle cache miss — fetching from API".into());
    let secrets = match fetch_secrets(&state, ctx.project_id).await { Ok(s) => s, Err(r) => return r };

    let bundle = match resolver.cold_fetch(&ctx.function_id, &tracer).await {
        Ok(b)  => b,
        Err(r) => return r,
    };

    // Set code_sha now that the bundle is resolved.
    tracer.code_sha = Some(match &bundle {
        ResolvedBundle::Deno { code, .. } => bundle_sha(code.as_bytes()),
        ResolvedBundle::Wasm { bytes }    => bundle_sha(bytes),
    });

    runner.run(bundle, secrets, &ctx, &tracer, start).await
}

// ── Private helpers ───────────────────────────────────────────────────────────

async fn fetch_secrets(
    state:      &AppState,
    project_id: Option<uuid::Uuid>,
) -> Result<std::collections::HashMap<String, String>, axum::response::Response> {
    state.secrets_client.fetch_secrets(project_id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR,
         Json(serde_json::json!({ "error": "SecretFetchError", "message": e })))
            .into_response()
    })
}
