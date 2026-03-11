use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::engine::{auth_context::AuthContext, error::EngineError};

/// All table events that can trigger a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    BeforeInsert,
    AfterInsert,
    BeforeUpdate,
    AfterUpdate,
    BeforeDelete,
    AfterDelete,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeforeInsert  => "before_insert",
            Self::AfterInsert   => "after_insert",
            Self::BeforeUpdate  => "before_update",
            Self::AfterUpdate   => "after_update",
            Self::BeforeDelete  => "before_delete",
            Self::AfterDelete   => "after_delete",
        }
    }

    /// Whether this is a "before" hook. Before hooks can abort the operation.
    pub fn is_before(self) -> bool {
        matches!(
            self,
            Self::BeforeInsert | Self::BeforeUpdate | Self::BeforeDelete
        )
    }
}

pub struct HookEngine;

impl HookEngine {
    /// Run all enabled hooks for `(table, event)`.
    ///
    /// **before_* hooks** — if the runtime call fails or returns a non-2xx
    /// response, the error is returned and the caller must abort the operation.
    ///
    /// **after_* hooks** — runtime errors are logged but do not fail the
    /// request (the data mutation has already committed).
    ///
    /// `request_id` is forwarded as `x-request-id` to the runtime so the
    /// hook invocation appears in the same trace as the originating request.
    pub async fn run(
        pool: &PgPool,
        http: &reqwest::Client,
        runtime_url: &str,
        auth: &AuthContext,
        table: &str,
        event: HookEvent,
        record: &serde_json::Value,
        request_id: &str,
    ) -> Result<(), EngineError> {
        let hooks = load_hooks(pool, auth.tenant_id, auth.project_id, table, event).await?;

        for function_id in hooks {
            let result = invoke_hook(http, runtime_url, auth, function_id, table, event, record, request_id).await;

            match result {
                Ok(()) => {
                    tracing::debug!(
                        %function_id,
                        event = event.as_str(),
                        "hook invoked successfully"
                    );
                }
                Err(e) => {
                    if event.is_before() {
                        tracing::error!(
                            %function_id,
                            event = event.as_str(),
                            error = %e,
                            "before hook rejected request"
                        );
                        return Err(e);
                    } else {
                        // After hooks must not break the user-facing response.
                        tracing::warn!(
                            %function_id,
                            event = event.as_str(),
                            error = %e,
                            "after hook failed (non-fatal)"
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Load the function_id(s) registered for `(table, event)` for this project.
async fn load_hooks(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    table: &str,
    event: HookEvent,
) -> Result<Vec<Uuid>, EngineError> {
    let rows = sqlx::query(
        "SELECT function_id FROM fluxbase_internal.hooks \
         WHERE tenant_id = $1 AND project_id = $2 \
           AND table_name = $3 AND event = $4 AND enabled = true \
         ORDER BY created_at",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(table)
    .bind(event.as_str())
    .fetch_all(pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(rows.iter().map(|r| r.get::<Uuid, _>("function_id")).collect())
}

/// POST to the runtime's /internal/execute endpoint.
/// Forwards `request_id` as `x-request-id` so the hook appears in the trace.
async fn invoke_hook(
    http: &reqwest::Client,
    runtime_url: &str,
    auth: &AuthContext,
    function_id: Uuid,
    table: &str,
    event: HookEvent,
    record: &serde_json::Value,
    request_id: &str,
) -> Result<(), EngineError> {
    let endpoint = format!("{}/internal/execute", runtime_url.trim_end_matches('/'));

    let payload = json!({
        "event":      event.as_str(),
        "table":      table,
        "record":     record,
        "auth": {
            "tenant_id":    auth.tenant_id,
            "project_id":   auth.project_id,
            "user_id":      auth.user_id,
            "role":         auth.role,
        },
    });

    let response = http
        .post(&endpoint)
        .header("x-request-id", request_id)
        .json(&json!({
            "function_id": function_id,
            "payload": payload,
        }))
        .send()
        .await
        .map_err(|e| EngineError::Internal(anyhow::anyhow!("hook HTTP error: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(EngineError::Internal(anyhow::anyhow!(
            "hook function {} returned {}: {}",
            function_id,
            status,
            body
        )));
    }

    Ok(())
}
