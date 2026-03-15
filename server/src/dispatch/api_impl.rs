//! In-process implementation of [`ApiDispatch`].
//!
//! Calls the API service functions directly using the shared DB pool \u2014
//! no HTTP round-trips, no serialization overhead.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::{ApiDispatch, ResolvedFunction};
use api::AppState as ApiState;

/// Calls `api` crate internals directly — used by the monolithic server binary.
pub struct InProcessApiDispatch {
    pub state: Arc<ApiState>,
}

#[async_trait]
impl ApiDispatch for InProcessApiDispatch {
    async fn get_bundle(&self, function_id: &str) -> Result<Value, String> {
        #[derive(sqlx::FromRow)]
        struct BundleRow {
            id:            Uuid,
            name:          String,
            runtime:       String,
            input_schema:  Option<Value>,
            output_schema: Option<Value>,
        }

        let pool = &self.state.pool;

        let row: Option<BundleRow> = if let Ok(fid) = function_id.parse::<Uuid>() {
            sqlx::query_as::<_, BundleRow>(
                "SELECT d.id, f.name, f.runtime, \
                        f.input_schema, f.output_schema \
                 FROM flux.deployments d \
                 JOIN flux.functions f ON f.id = d.function_id \
                 WHERE d.function_id = $1 AND d.is_active = true \
                 ORDER BY d.version DESC LIMIT 1",
            )
            .bind(fid)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("bundle DB query failed: {}", e))?
        } else {
            sqlx::query_as::<_, BundleRow>(
                "SELECT d.id, f.name, f.runtime, \
                        f.input_schema, f.output_schema \
                 FROM flux.deployments d \
                 JOIN flux.functions f ON f.id = d.function_id \
                 WHERE f.name = $1 AND d.is_active = true \
                 ORDER BY d.version DESC LIMIT 1",
            )
            .bind(function_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("bundle DB query failed: {}", e))?
        };

        let r = row.ok_or_else(|| "HTTP 404: no active deployment found".to_string())?;

        // Read bundle from filesystem — bundles live at {FLUX_FUNCTIONS_DIR}/{name}.{ext}
        let ext = if r.runtime == "wasm" { "wasm" } else { "js" };
        let functions_dir = &self.state.functions_dir;
        let bundle_path = std::path::Path::new(functions_dir).join(format!("{}.{}", r.name, ext));

        // WASM bundles are binary — base64-encode for JSON transport.
        // JS bundles are UTF-8 text — read directly.
        let code = if r.runtime == "wasm" {
            let bytes = std::fs::read(&bundle_path).map_err(|e| {
                format!("HTTP 404: bundle file '{}' not found on filesystem: {}", bundle_path.display(), e)
            })?;
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        } else {
            std::fs::read_to_string(&bundle_path).map_err(|e| {
                format!("HTTP 404: bundle file '{}' not found on filesystem: {}", bundle_path.display(), e)
            })?
        };

        Ok(serde_json::json!({
            "deployment_id": r.id,
            "runtime":       r.runtime,
            "code":          code,
            "input_schema":  r.input_schema,
            "output_schema": r.output_schema,
        }))
    }

    async fn write_log(&self, entry: Value) -> Result<(), String> {
        let pool = &self.state.pool;

        let level    = entry.get("level")   .and_then(|v| v.as_str()).unwrap_or("info");
        let source   = entry.get("source")  .and_then(|v| v.as_str()).unwrap_or("function");
        let message  = entry.get("message") .and_then(|v| v.as_str()).unwrap_or("");
        let resource = entry.get("resource_id").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = entry.get("request_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let span_type  = entry.get("span_type") .and_then(|v| v.as_str()).map(|s| s.to_string());
        let metadata   = entry.get("metadata").cloned();

        // Note: tenant_id and project_id were removed from platform_logs by
        // migration 20260314000042_drop_tenant_project.sql — do not include them.
        sqlx::query(
            "INSERT INTO flux.platform_logs \
             (source, resource_id, level, message, request_id, metadata, span_type) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(source)
        .bind(resource)
        .bind(level)
        .bind(message)
        .bind(&request_id)
        .bind(&metadata)
        .bind(&span_type)
        .execute(pool)
        .await
        .map_err(|e| format!("log insert failed: {}", e))?;

        Ok(())
    }

    async fn write_network_call(&self, call: Value) -> Result<(), String> {
        let pool = &self.state.pool;

        let request_id       = call.get("request_id")      .and_then(|v| v.as_str()).unwrap_or("");
        let span_id          = call.get("span_id")         .and_then(|v| v.as_str());
        let call_seq         = call.get("call_seq")        .and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let method           = call.get("method")          .and_then(|v| v.as_str()).unwrap_or("GET");
        let url              = call.get("url")             .and_then(|v| v.as_str()).unwrap_or("");
        let host             = call.get("host")            .and_then(|v| v.as_str()).unwrap_or("");
        let request_headers  = call.get("request_headers") .cloned();
        let request_body     = call.get("request_body")    .and_then(|v| v.as_str()).map(|s| s.to_string());
        let status           = call.get("status")          .and_then(|v| v.as_i64()).map(|s| s as i32);
        let response_headers = call.get("response_headers").cloned();
        let response_body    = call.get("response_body")   .and_then(|v| v.as_str()).map(|s| s.to_string());
        let duration_ms      = call.get("duration_ms")     .and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let error            = call.get("error")           .and_then(|v| v.as_str()).map(|s| s.to_string());

        sqlx::query(
            "INSERT INTO flux_internal.network_calls \
             (request_id, span_id, call_seq, method, url, host, \
              request_headers, request_body, status, response_headers, response_body, \
              duration_ms, error) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(request_id)
        .bind(span_id)
        .bind(call_seq)
        .bind(method)
        .bind(url)
        .bind(host)
        .bind(&request_headers)
        .bind(&request_body)
        .bind(status)
        .bind(&response_headers)
        .bind(&response_body)
        .bind(duration_ms)
        .bind(&error)
        .execute(pool)
        .await
        .map_err(|e| format!("network_call insert failed: {}", e))?;

        Ok(())
    }

    async fn get_secrets(&self) -> Result<HashMap<String, String>, String> {
        api::secrets::service::get_runtime_secrets(
            &self.state.pool,
        )
        .await
        .map_err(|e| format!("secrets fetch failed: {:?}", e))
    }

    async fn resolve_function(
        &self,
        name: &str,
    ) -> Result<ResolvedFunction, String> {
        #[derive(sqlx::FromRow)]
        struct Row { id: Uuid }

        let pool = &self.state.pool;

        let row: Option<Row> = if let Ok(fid) = name.parse::<Uuid>() {
            sqlx::query_as::<_, Row>(
                "SELECT id FROM flux.functions WHERE id = $1",
            )
            .bind(fid)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("resolve_function DB query failed: {}", e))?
        } else {
            sqlx::query_as::<_, Row>(
                "SELECT id FROM flux.functions WHERE name = $1",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("resolve_function DB query failed: {}", e))?
        };

        let r = row.ok_or_else(|| format!("function '{}' not found", name))?;

        Ok(ResolvedFunction {
            function_id: r.id,
        })
    }
}
