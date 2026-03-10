/// Tool Executor — the single execution layer for all tool invocations.
///
/// Design rule:
///   Function  → ToolExecutor.run()
///   Workflow  → ToolExecutor.run()   (Phase 2)
///   Agent     → ToolExecutor.run()   (Phase 3)
///
/// Nothing bypasses this. One path, full trace visibility.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

use super::registry::ToolRegistry;
use super::composio;

/// A single tool execution record (emitted as a trace span).
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolSpan {
    /// Fluxbase tool name: "slack.send_message"
    pub tool:        String,
    /// Composio action resolved for this execution
    pub action:      String,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Whether the tool call succeeded
    pub success:     bool,
    /// Error message if not successful
    pub error:       Option<String>,
}

/// Output from a tool execution.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolOutput {
    pub data:    Option<Value>,
    pub span:    ToolSpan,
}

/// The executor is cheap to create (holds only Arc refs).
/// It is instantiated once per function execution and injected into the JS ctx.
pub struct ToolExecutor {
    registry:    ToolRegistry,
    /// Fluxbase platform Composio API key (set once per Fluxbase deployment)
    api_key:     Option<String>,
    /// Tenant identifier — maps to a Composio entity so each tenant's
    /// connected accounts are isolated
    entity_id:   String,
}

impl ToolExecutor {
    pub fn new(api_key: Option<String>, entity_id: String) -> Self {
        Self {
            registry: ToolRegistry::new(),
            api_key,
            entity_id,
        }
    }

    /// Execute a tool by Fluxbase name.
    ///
    /// This is the one method all callers use:
    ///   ctx.tools.run("slack.send_message", { channel: "#ops", text: "…" })
    pub async fn run(
        &self,
        tool_name: &str,
        input:     Value,
    ) -> Result<ToolOutput, String> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            "tools_not_configured: Composio integration is not available on this runtime. \
             Contact support if you believe this is an error.".to_string()
        })?;

        // Resolve the Composio action ID through the registry
        let composio_action = self.registry.resolve_composio_action(tool_name);

        let start = Instant::now();

        let result = composio::execute_action(
            api_key,
            &self.entity_id,
            &composio_action,
            input,
        ).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => Ok(ToolOutput {
                data: output.data,
                span: ToolSpan {
                    tool:        tool_name.to_string(),
                    action:      composio_action,
                    duration_ms,
                    success:     true,
                    error:       None,
                },
            }),
            Err(e) => Err(format!("tool:{}  error: {}", tool_name, e)),
        }
    }

    /// List all available tools (used by CLI `flux tools list` and dashboard).
    pub fn list_tools(&self) -> Vec<serde_json::Value> {
        self.registry.all().iter().map(|meta| {
            serde_json::json!({
                "name":        meta.name,
                "label":       meta.label,
                "app":         meta.app,
                "description": meta.description,
            })
        }).collect()
    }
}

/// State injected into the Deno OpState so the op_execute_tool op can
/// call back into the executor without holding a Rust reference across an
/// await boundary.
pub struct ToolOpState {
    pub api_key:   Option<String>,
    pub entity_id: String,
}
