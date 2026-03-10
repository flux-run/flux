/// Agent Runtime — Phase 3 of the Fluxbase execution stack.
///
/// Agents are LLM-driven loops that autonomously select and call tools until
/// a goal is achieved.  The agent loop sits entirely in the JavaScript sandbox;
/// the only new Rust element is op_agent_llm_call — a single op that calls an
/// OpenAI-compatible LLM endpoint and returns the next action decision.
///
/// The single execution rule is preserved:
///   Agent → op_agent_llm_call (decide) → ctx.tools.run() → ToolExecutor → Composio
///
/// Configuration secrets:
///   FLUXBASE_LLM_KEY   — API key for the LLM provider  (required for agents)
///   FLUXBASE_LLM_URL   — chat completions endpoint      (default: OpenAI)
///   FLUXBASE_LLM_MODEL — model name                     (default: gpt-4o-mini)

pub mod llm;

/// Agent LLM state injected into Deno's OpState for each function execution.
pub struct AgentOpState {
    /// LLM API key  (FLUXBASE_LLM_KEY tenant secret)
    pub llm_key:   Option<String>,
    /// Chat completions endpoint  (OpenAI-compatible)
    pub llm_url:   String,
    /// Model identifier
    pub llm_model: String,
}
