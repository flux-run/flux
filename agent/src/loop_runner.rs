//! Core agent execution loop.
//!
//! Flow per turn:
//!   1. Call LLM with current message history + tool schemas.
//!   2. Record the step to platform_logs.
//!   3a. If ToolUse → check rules → dispatch to runtime → append messages → continue.
//!   3b. If FinalAnswer → return output.
//!   4. If max_turns exhausted → return MaxTurnsExceeded error.

use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::{ExecuteRequest, RuntimeDispatch};

use crate::llm::{ChatMessage, ChatRequest, LlmResponse};
use crate::recording::{record_step, StepRecord};
use crate::rules::RuleState;
use crate::schema::AgentDefinition;
use crate::AgentError;

pub struct AgentResult {
    pub output: String,
    pub turns:  u32,
}

pub async fn run_agent(
    agent:            &AgentDefinition,
    input:            Value,
    request_id:       &str,
    project_id:       Uuid,
    llm_key:          &str,
    tool_schemas:     &[Value],
    runtime_dispatch: &dyn RuntimeDispatch,
    pool:             &sqlx::PgPool,
) -> Result<Value, AgentError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| AgentError::Llm(e.to_string()))?;

    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage::system(&agent.system),
        ChatMessage::user(&input.to_string()),
    ];

    let mut rules = RuleState::new(&agent.rules);
    let (top_p, max_tokens) = agent.config.as_ref()
        .map(|c| (c.top_p, c.max_tokens))
        .unwrap_or((None, None));

    for turn in 0..agent.max_turns {
        // ── Call LLM ─────────────────────────────────────────────────────
        let llm_resp = crate::llm::chat(
            &http,
            &agent.llm_url,
            llm_key,
            ChatRequest {
                model:       &agent.model,
                messages:    &messages,
                tools:       tool_schemas,
                temperature: agent.temperature,
                top_p,
                max_tokens,
            },
        )
        .await
        .map_err(AgentError::Llm)?;

        match llm_resp {
            // ── Tool call(s) ─────────────────────────────────────────────
            LlmResponse::ToolUse { calls, prompt_tokens, completion_tokens } => {
                // Record turn to platform_logs
                let tool_names = calls.iter()
                    .map(|c| c.function.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                let _ = record_step(pool, &StepRecord {
                    request_id, project_id,
                    agent_name:        &agent.name,
                    model:             &agent.model,
                    turn,
                    tool_choice:       Some(&tool_names),
                    prompt_tokens,
                    completion_tokens,
                    level:             "info",
                    message:           &format!("turn {} → tools: {}", turn + 1, tool_names),
                }).await;

                // Append the assistant tool-use message
                messages.push(ChatMessage::assistant_tool_calls(calls.clone()));

                // Execute each tool call in sequence
                for call in &calls {
                    let tool_name = &call.function.name;

                    // ── Guard-rail check ─────────────────────────────────
                    rules.check(tool_name).map_err(AgentError::RuleViolation)?;

                    // ── Parse arguments ──────────────────────────────────
                    let payload: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or(serde_json::json!({}));

                    tracing::debug!(
                        agent = %agent.name,
                        turn,
                        tool = tool_name,
                        "dispatching tool"
                    );

                    // ── Dispatch to runtime ──────────────────────────────
                    let exec_resp = runtime_dispatch.execute(ExecuteRequest {
                        function_id:    tool_name.clone(),
                        project_id:     Some(project_id),
                        payload,
                        execution_seed: None,
                        request_id:     Some(request_id.to_string()),
                        parent_span_id: None,
                        runtime_hint:   None,
                        user_id:        None,
                        jwt_claims:     None,
                    })
                    .await
                    .map_err(|e| AgentError::Dispatch(format!("{}: {}", tool_name, e)))?;

                    rules.record(tool_name);

                    // Serialise tool output for the next LLM turn
                    let output_str = serde_json::to_string(&exec_resp.body)
                        .unwrap_or_else(|_| exec_resp.body.to_string());

                    messages.push(ChatMessage::tool_result(
                        &call.id, tool_name, &output_str,
                    ));
                }
            }

            // ── Final answer ─────────────────────────────────────────────
            LlmResponse::FinalAnswer { content, prompt_tokens, completion_tokens } => {
                let _ = record_step(pool, &StepRecord {
                    request_id, project_id,
                    agent_name:        &agent.name,
                    model:             &agent.model,
                    turn,
                    tool_choice:       None,
                    prompt_tokens,
                    completion_tokens,
                    level:             "info",
                    message:           &format!("turn {} → final answer", turn + 1),
                }).await;

                messages.push(ChatMessage::assistant_text(&content));

                tracing::info!(
                    agent = %agent.name,
                    turns = turn + 1,
                    "agent completed"
                );

                // Return structured output if the answer is valid JSON,
                // otherwise wrap in { "answer": "..." }
                let output: Value = serde_json::from_str(&content)
                    .unwrap_or_else(|_| serde_json::json!({ "answer": content }));

                return Ok(output);
            }
        }
    }

    Err(AgentError::MaxTurnsExceeded(agent.max_turns))
}
