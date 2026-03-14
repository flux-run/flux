//! In-process execution entry point.
//!
//! Used by the monolithic `server` binary's `InProcessRuntimeDispatch`.
//! Performs the same logic as `execute_handler` but returns an `ExecuteResponse`
//! (plain data) instead of an axum `Response`, so no HTTP is involved.

use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use job_contract::dispatch::{ExecuteRequest, ExecuteResponse};

use crate::AppState;
use crate::execute::bundle::{BundleResolver, ResolvedBundle, bundle_sha};
use crate::execute::runner::{ExecutionRunner, allowed_wasm_http_hosts, project_schema_name};
use crate::execute::types::InvocationCtx;
use crate::trace::emitter::TraceEmitter;

/// Execute a function in-process and return the structured response.
///
/// This is the single-binary alternative to the HTTP `POST /execute` handler.
pub async fn invoke(
    state: Arc<AppState>,
    req:   ExecuteRequest,
) -> Result<ExecuteResponse, String> {
    let seed_bytes = Uuid::new_v4().into_bytes();
    let execution_seed = req.execution_seed.unwrap_or_else(|| i64::from_le_bytes([
        seed_bytes[0], seed_bytes[1], seed_bytes[2], seed_bytes[3],
        seed_bytes[4], seed_bytes[5], seed_bytes[6], seed_bytes[7],
    ]));

    let ctx = InvocationCtx {
        function_id:    req.function_id,
        project_id:     req.project_id,
        payload:        req.payload,
        execution_seed,
        request_id:     req.request_id.clone(),
        parent_span_id: req.parent_span_id.clone(),
    };

    let mut tracer = TraceEmitter::new(
        Arc::clone(&state.api),
        ctx.function_id.clone(),
        ctx.project_id,
        ctx.request_id.clone(),
        ctx.parent_span_id.clone(),
    );

    let start = Instant::now();
    let resolver = BundleResolver {
        bundle_cache: &state.bundle_cache,
        schema_cache: &state.schema_cache,
        wasm_pool:    &state.wasm_pool,
        http_client:  &state.http_client,
        api:          &*state.api,
    };
    let runner = ExecutionRunner {
        isolate_pool:    &state.isolate_pool,
        wasm_pool:       &state.wasm_pool,
        schema_cache:    &state.schema_cache,
        http_client:     &state.http_client,
        wasm_http_hosts: allowed_wasm_http_hosts(),
        queue_url:       &state.queue_url,
        api_url:         &state.api_url,
        service_token:   &state.service_token,
        data_engine_url: &state.data_engine_url,
        database:        project_schema_name(ctx.project_id),
        runtime_url:     &state.runtime_url,
    };

    let hints = req.runtime_hint.as_deref().unwrap_or("");

    // ── Warm WASM path ────────────────────────────────────────────────────
    if hints != "deno" {
        if let Some(wasm_bytes) = resolver.warm_wasm(&ctx.function_id).await {
            tracer.code_sha = Some(bundle_sha(&wasm_bytes));
            let secrets = state.secrets_client.fetch_secrets(ctx.project_id).await?;
            let (status, body) = runner.run(
                ResolvedBundle::Wasm { bytes: wasm_bytes.to_vec() },
                secrets, &ctx, &tracer, start,
            ).await;
            return Ok(ExecuteResponse { body, status: status.as_u16(), duration_ms: 0 });
        }
    }

    // ── Warm Deno path ────────────────────────────────────────────────────
    if hints != "wasm" {
        if let Some(cached_code) = resolver.warm_deno(&ctx.function_id) {
            tracer.code_sha = Some(bundle_sha(cached_code.as_bytes()));
            let secrets = state.secrets_client.fetch_secrets(ctx.project_id).await?;
            let (status, body) = runner.run(
                ResolvedBundle::Deno { code: cached_code },
                secrets, &ctx, &tracer, start,
            ).await;
            return Ok(ExecuteResponse { body, status: status.as_u16(), duration_ms: 0 });
        }
    }

    // ── Cold path ─────────────────────────────────────────────────────────
    let secrets = state.secrets_client.fetch_secrets(ctx.project_id).await?;
    let bundle = resolver.cold_fetch(&ctx.function_id, &tracer).await
        .map_err(|r| format!("bundle error: HTTP {}", r.status().as_u16()))?;

    tracer.code_sha = Some(match &bundle {
        ResolvedBundle::Deno { code }  => bundle_sha(code.as_bytes()),
        ResolvedBundle::Wasm { bytes } => bundle_sha(bytes),
    });

    let (status, body) = runner.run(bundle, secrets, &ctx, &tracer, start).await;
    Ok(ExecuteResponse { body, status: status.as_u16(), duration_ms: 0 })
}
