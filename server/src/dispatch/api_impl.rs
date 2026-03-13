//! In-process implementation of [`ApiDispatch`].
//!
//! Calls the API service functions directly using the shared DB pool and
//! storage backend — no HTTP round-trips, no serialization overhead.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::ApiDispatch;
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
            bundle_code:   Option<String>,
            bundle_url:    Option<String>,
            runtime:       String,
            input_schema:  Option<Value>,
            output_schema: Option<Value>,
        }

        let pool = &self.state.pool;

        let row: Option<BundleRow> = if let Ok(fid) = function_id.parse::<Uuid>() {
            sqlx::query_as::<_, BundleRow>(
                "SELECT d.id, d.bundle_code, d.bundle_url, f.runtime, \
                        f.input_schema, f.output_schema \
                 FROM deployments d \
                 JOIN functions f ON f.id = d.function_id \
                 WHERE d.function_id = $1 AND d.is_active = true \
                 ORDER BY d.version DESC LIMIT 1",
            )
            .bind(fid)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("bundle DB query failed: {}", e))?
        } else {
            sqlx::query_as::<_, BundleRow>(
                "SELECT d.id, d.bundle_code, d.bundle_url, f.runtime, \
                        f.input_schema, f.output_schema \
                 FROM deployments d \
                 JOIN functions f ON f.id = d.function_id \
                 WHERE f.name = $1 AND d.is_active = true \
                 ORDER BY d.version DESC LIMIT 1",
            )
            .bind(function_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("bundle DB query failed: {}", e))?
        };

        match row {
            Some(r) => {
                // In local mode always prefer inline bundle_code; presigned S3 URLs
                // point to minio which isn't running during `flux dev`.
                let use_inline = self.state.storage.local_mode || r.bundle_url.is_none();

                if !use_inline {
                    let s3_key = r.bundle_url.as_deref().unwrap();
                    let url = self.state.storage
                        .presigned_get_object(s3_key, std::time::Duration::from_secs(300))
                        .await
                        .map_err(|e| format!("presign failed: {}", e))?;
                    Ok(serde_json::json!({
                        "deployment_id": r.id,
                        "runtime":       r.runtime,
                        "url":           url,
                        "input_schema":  r.input_schema,
                        "output_schema": r.output_schema,
                    }))
                } else if let Some(code) = r.bundle_code {
                    Ok(serde_json::json!({
                        "deployment_id": r.id,
                        "runtime":       r.runtime,
                        "code":          code,
                        "input_schema":  r.input_schema,
                        "output_schema": r.output_schema,
                    }))
                } else {
                    Err("HTTP 404: no bundle found for this function".to_string())
                }
            }
            None => Err("HTTP 404: no active deployment found".to_string()),
        }
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

        // tenant_id: prefer explicit field, fall back to local_tenant_id
        let tenant_id = entry.get("tenant_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<Uuid>().ok())
            .unwrap_or(self.state.local_tenant_id);

        let project_id = entry.get("project_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<Uuid>().ok());

        sqlx::query(
            "INSERT INTO platform_logs \
             (tenant_id, project_id, source, resource_id, level, message, \
              request_id, metadata, span_type) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(tenant_id)
        .bind(project_id)
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

    async fn get_secrets(
        &self,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String> {
        api::secrets::service::get_runtime_secrets(
            &self.state.pool,
            self.state.local_tenant_id,
            project_id,
        )
        .await
        .map_err(|e| format!("secrets fetch failed: {:?}", e))
    }
}
