use reqwest::Client;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::events::dispatcher;

/// Trigger all workflows whose `trigger_event` pattern matches the given event.
///
/// Called from the event dispatcher as a `queue_job`-like integration, or
/// directly from the event worker after a successful emit.
pub struct WorkflowEngine;

impl WorkflowEngine {
    pub async fn trigger(
        pool: &PgPool,
        http: &Client,
        runtime_url: &str,
        tenant_id: Uuid,
        project_id: Uuid,
        event_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
    ) {
        if let Err(e) = Self::do_trigger(
            pool, http, runtime_url, tenant_id, project_id,
            event_id, event_type, payload,
        )
        .await
        {
            tracing::warn!(error = %e, event_type = %event_type, "workflow trigger failed (non-fatal)");
        }
    }

    async fn do_trigger(
        pool: &PgPool,
        http: &Client,
        runtime_url: &str,
        tenant_id: Uuid,
        project_id: Uuid,
        event_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        let table_wildcard = event_type
            .split_once('.')
            .map(|(t, _)| format!("{}.*", t))
            .unwrap_or_else(|| "*".to_string());

        let workflows = sqlx::query(
            "SELECT id FROM fluxbase_internal.workflows \
             WHERE tenant_id = $1 AND project_id = $2 \
               AND enabled = TRUE \
               AND trigger_event IN ($3, $4, '*')",
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(event_type)
        .bind(&table_wildcard)
        .fetch_all(pool)
        .await?;

        for wf in &workflows {
            let workflow_id: Uuid = wf.get("id");
            // Create execution record.
            let exec_row = sqlx::query(
                "INSERT INTO fluxbase_internal.workflow_executions \
                     (workflow_id, trigger_event_id, status, context) \
                 VALUES ($1, $2, 'running', $3) \
                 RETURNING id",
            )
            .bind(workflow_id)
            .bind(event_id)
            .bind(payload)
            .fetch_one(pool)
            .await?;

            let exec_id: Uuid = exec_row.get("id");
            tracing::debug!(workflow_id = %workflow_id, execution_id = %exec_id, "workflow triggered");
        }

        Ok(())
    }
}

/// Background worker: advance running workflow executions one step at a time.
pub async fn run(pool: std::sync::Arc<PgPool>, http: std::sync::Arc<Client>, runtime_url: String) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    tracing::info!("workflow worker started");

    loop {
        tick.tick().await;
        if let Err(e) = advance_executions(&pool, &http, &runtime_url).await {
            tracing::warn!(error = %e, "workflow worker error");
        }
    }
}

async fn advance_executions(pool: &PgPool, http: &Client, runtime_url: &str) -> Result<(), sqlx::Error> {
    // Claim running executions that have a pending step to dispatch.
    let execs = sqlx::query(
        "SELECT e.id, e.workflow_id, e.current_step, e.context \
         FROM fluxbase_internal.workflow_executions e \
         WHERE e.status = 'running' \
         LIMIT 20 \
         FOR UPDATE OF e SKIP LOCKED",
    )
    .fetch_all(pool)
    .await?;

    for exec in &execs {
        let exec_id: Uuid = exec.get("id");
        let workflow_id: Uuid = exec.get("workflow_id");
        let current_step: i32 = exec.get("current_step");
        let context: serde_json::Value = exec.get("context");

        // Load the step.
        let step = sqlx::query(
            "SELECT id, action_type, action_config \
             FROM fluxbase_internal.workflow_steps \
             WHERE workflow_id = $1 AND step_order = $2",
        )
        .bind(workflow_id)
        .bind(current_step)
        .fetch_optional(pool)
        .await?;

        match step {
            None => {
                // No more steps — workflow complete.
                sqlx::query(
                    "UPDATE fluxbase_internal.workflow_executions \
                     SET status = 'done', finished_at = now() WHERE id = $1",
                )
                .bind(exec_id)
                .execute(pool)
                .await?;
            }
            Some(s) => {
                let action_type: String = s.get("action_type");
                let action_config: serde_json::Value = s.get("action_config");

                let step_id: Uuid = s.get("id");
                let result = dispatcher::dispatch(
                    pool, http, runtime_url,
                    step_id,          // reuse subscription_id param — it's unused
                    &action_type,
                    &action_config,
                    &context,
                    "workflow.step",
                )
                .await;

                match result {
                    Ok(_) => {
                        // Advance to next step.
                        sqlx::query(
                            "UPDATE fluxbase_internal.workflow_executions \
                             SET current_step = current_step + 1 WHERE id = $1",
                        )
                        .bind(exec_id)
                        .execute(pool)
                        .await?;
                    }
                    Err(e) => {
                        sqlx::query(
                            "UPDATE fluxbase_internal.workflow_executions \
                             SET status = 'failed', error_message = $1, finished_at = now() \
                             WHERE id = $2",
                        )
                        .bind(e.as_str())
                        .bind(exec_id)
                        .execute(pool)
                        .await?;
                        tracing::warn!(exec_id = %exec_id, error = %e, "workflow step failed");
                    }
                }
            }
        }
    }

    Ok(())
}
