//! Write agent step spans to `platform_logs`.
//!
//! Each turn of the agent loop emits one row with:
//!   source      = "agent"
//!   resource_id = <agent name>
//!   span_type   = "agent_step"
//!   level       = "info"  (or "error" if the step failed)
//!   message     = brief human-readable summary
//!   metadata    = { model, turn, prompt_tokens, completion_tokens, tool_choice }

use sqlx::PgPool;
use uuid::Uuid;

pub struct StepRecord<'a> {
    pub request_id:        &'a str,
    pub project_id:        Uuid,
    pub agent_name:        &'a str,
    pub model:             &'a str,
    pub turn:              u32,
    pub tool_choice:       Option<&'a str>,   // tool name, or None for final answer
    pub prompt_tokens:     u32,
    pub completion_tokens: u32,
    pub level:             &'a str,    // "info" | "error"
    pub message:           &'a str,
}

pub async fn record_step(pool: &PgPool, step: &StepRecord<'_>) -> Result<(), String> {
    let metadata = serde_json::json!({
        "model":             step.model,
        "turn":              step.turn,
        "prompt_tokens":     step.prompt_tokens,
        "completion_tokens": step.completion_tokens,
        "tool_choice":       step.tool_choice,
    });

    sqlx::query(
        "INSERT INTO platform_logs
             (tenant_id, project_id, source, resource_id, level, message,
              request_id, metadata, span_type)
         SELECT t.id, $1, 'agent', $2, $3, $4, $5, $6, 'agent_step'
         FROM projects p
         JOIN tenants t ON t.id = p.tenant_id
         WHERE p.id = $1
         LIMIT 1",
    )
    .bind(step.project_id)
    .bind(step.agent_name)
    .bind(step.level)
    .bind(step.message)
    .bind(step.request_id)
    .bind(&metadata)
    .execute(pool)
    .await
    .map_err(|e| format!("recording: {}", e))?;

    Ok(())
}
