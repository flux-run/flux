//! Flux Agent — pure-Rust LLM orchestration engine.
//!
//! An agent is a YAML-defined orchestrator that:
//!   1. Receives a JSON input.
//!   2. Calls an OpenAI-compatible LLM with the system prompt + tool schemas.
//!   3. Dispatches tool calls to user functions via `RuntimeDispatch`.
//!   4. Loops until the LLM returns a final answer or `max_turns` is reached.
//!   5. Writes each step to `platform_logs` as agent_step spans.
//!
//! LLM config:
//!   `llm_url`    — chat completions endpoint (stored in agent definition)
//!   `llm_secret` — name of the project secret that holds the API key
//!                  (e.g. "OPENAI_KEY" → reads `secrets["OPENAI_KEY"]`)
//!
//! Tools = user functions.  Description and parameter schema are read from
//! the `functions` table (`description`, `input_schema` columns).

pub mod llm;
pub mod loop_runner;
pub mod recording;
pub mod registry;
pub mod rules;
pub mod schema;
pub mod tools;

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;
use uuid::Uuid;

use job_contract::dispatch::RuntimeDispatch;

/// Shared state injected into the server and passed to every agent run.
pub struct AgentState {
    pub pool:             PgPool,
    pub runtime_dispatch: Arc<dyn RuntimeDispatch>,
}

/// Top-level entry point.  Loads agent definition, resolves LLM key from
/// secrets, and runs the execution loop.
pub async fn run(
    state:      &AgentState,
    name:       &str,
    input:      serde_json::Value,
    request_id: &str,
    project_id: Uuid,
    secrets:    &HashMap<String, String>,
) -> Result<serde_json::Value, AgentError> {
    let agent = registry::get_agent(&state.pool, name, project_id).await
        .map_err(|e| AgentError::Registry(e.to_string()))?
        .ok_or_else(|| AgentError::NotFound(name.to_string()))?;

    let llm_key = secrets
        .get(&agent.llm_secret)
        .cloned()
        .ok_or_else(|| AgentError::MissingSecret(agent.llm_secret.clone()))?;

    let tool_schemas = tools::build_tool_schemas(&state.pool, &agent.tools)
        .await
        .map_err(|e| AgentError::Tools(e.to_string()))?;

    let result = loop_runner::run_agent(
        &agent,
        input,
        request_id,
        project_id,
        &llm_key,
        &tool_schemas,
        &*state.runtime_dispatch,
        &state.pool,
    )
    .await?;

    Ok(result)
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AgentError {
    NotFound(String),
    MissingSecret(String),
    Registry(String),
    Tools(String),
    Llm(String),
    RuleViolation(String),
    MaxTurnsExceeded(u32),
    Dispatch(String),
    Recording(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(n)        => write!(f, "agent_not_found: {}", n),
            Self::MissingSecret(s)   => write!(f, "missing_secret: {} — set it with `flux secrets set {s} <key>`", s),
            Self::Registry(e)        => write!(f, "registry_error: {}", e),
            Self::Tools(e)           => write!(f, "tools_error: {}", e),
            Self::Llm(e)             => write!(f, "llm_error: {}", e),
            Self::RuleViolation(e)   => write!(f, "rule_violation: {}", e),
            Self::MaxTurnsExceeded(n)=> write!(f, "max_turns_exceeded: agent ran {} turns without a final answer", n),
            Self::Dispatch(e)        => write!(f, "dispatch_error: {}", e),
            Self::Recording(e)       => write!(f, "recording_error: {}", e),
        }
    }
}

impl std::error::Error for AgentError {}
