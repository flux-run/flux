//! In-process implementation of [`ApiDispatch`].
//!
//! Calls the API service functions directly using the shared DB pool \u2014
//! no HTTP round-trips, no serialization overhead.

use std::collections::HashMap;

use async_trait::async_trait;
use base64::Engine;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use runtime::contracts::{ApiDispatch, ResolvedFunction};

/// Calls `api` crate internals directly — used by the monolithic server binary.
pub struct InProcessApiDispatch {
    pub pool: PgPool,
    pub functions_dir: String,
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

        let pool = &self.pool;

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
        let ext = "js";
        let functions_dir = &self.functions_dir;
        let bundle_path = std::path::Path::new(functions_dir).join(format!("{}.{}", r.name, ext));

        // JS bundles are UTF-8 text.
        let code = if bundle_path.exists() {
            std::fs::read_to_string(&bundle_path).map_err(|e| {
                format!("HTTP 404: bundle file '{}' not found on filesystem: {}", bundle_path.display(), e)
            })?
        } else {
            let (bytes, encoding): (Vec<u8>, String) = sqlx::query_as(
                "SELECT artifact_bytes, artifact_encoding FROM flux.deployments WHERE sha = $1 LIMIT 1",
            )
            .bind(function_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("deployment lookup failed: {}", e))?
            .ok_or_else(|| "HTTP 404: no active deployment found".to_string())?;

            match encoding.as_str() {
                "raw" => String::from_utf8(bytes)
                    .map_err(|e| format!("deployment artifact is not valid UTF-8: {e}"))?,
                "base64" => {
                    let decoded = base64::engine::general_purpose::STANDARD.decode(bytes)
                        .map_err(|e| format!("invalid base64 deployment artifact: {e}"))?;
                    String::from_utf8(decoded)
                        .map_err(|e| format!("decoded deployment artifact is not valid UTF-8: {e}"))?
                }
                other => return Err(format!("unsupported artifact_encoding: {}", other)),
            }
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
        let pool = &self.pool;

        let level    = entry.get("level")   .and_then(|v| v.as_str()).unwrap_or("info");
        let source   = entry.get("source")  .and_then(|v| v.as_str()).unwrap_or("function");
        let message  = entry.get("message") .and_then(|v| v.as_str()).unwrap_or("");
        let resource = entry.get("resource_id").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = entry.get("request_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let span_type  = entry.get("span_type") .and_then(|v| v.as_str()).map(|s| s.to_string());
        let metadata   = entry.get("metadata").cloned();

        // Note: tenant_id and project_id were removed from platform_logs by
        // migration 20260314000042_drop_tenant_project.sql — do not include them.
        if let Some(exec_id) = request_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok())
        {
            sqlx::query(
                "INSERT INTO flux.logs (execution_id, level, message) VALUES ($1, $2, $3)",
            )
            .bind(exec_id)
            .bind(level)
            .bind(message)
            .execute(pool)
            .await
            .map_err(|e| format!("log insert failed: {}", e))?;
        } else {
            let _ = (source, resource, metadata, span_type);
        }

        Ok(())
    }

    async fn write_network_call(&self, call: Value) -> Result<(), String> {
        let pool = &self.pool;

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

        if let Ok(exec_id) = Uuid::parse_str(request_id) {
            let request_payload = serde_json::json!({
                "call_seq": call_seq,
                "method": method,
                "url": url,
                "host": host,
                "headers": request_headers,
                "body": request_body,
                "span_id": span_id,
            });
            let response_payload = serde_json::json!({
                "status": status,
                "headers": response_headers,
                "body": response_body,
                "error": error,
            });

            sqlx::query(
                "INSERT INTO flux.checkpoints (execution_id, call_index, boundary, request, response, duration_ms)
                 VALUES ($1, $2, 'http', convert_to($3::text, 'UTF8'), convert_to($4::text, 'UTF8'), $5)",
            )
            .bind(exec_id)
            .bind(call_seq)
            .bind(request_payload.to_string())
            .bind(response_payload.to_string())
            .bind(duration_ms)
            .execute(pool)
            .await
            .map_err(|e| format!("network checkpoint insert failed: {}", e))?;
        }

        Ok(())
    }

    async fn get_secrets(&self) -> Result<HashMap<String, String>, String> {
        Ok(HashMap::new())
    }

    async fn resolve_function(
        &self,
        name: &str,
    ) -> Result<ResolvedFunction, String> {
        #[derive(sqlx::FromRow)]
        struct Row { id: Uuid }

        let pool = &self.pool;

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
