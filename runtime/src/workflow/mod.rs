/// Workflow Engine — Phase 2 of the Fluxbase execution stack.
///
/// Workflows are sequential (or parallel) chains of named steps.
/// Each step receives the full ctx and the previous step outputs.
///
/// The engine is implemented in the JavaScript sandbox so that step functions
/// are normal JS closures.  No new Rust execution path is introduced.
///
/// The single execution rule is preserved:
///   Workflow → step.fn(ctx, prev) → ctx.tools.run() → ToolExecutor → Composio
///
/// Rust-side: only metadata types kept for future persistence/replay.

use serde::{Deserialize, Serialize};

/// Step execution record (used for future persistence / replay).
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowStepRecord {
    pub name:        String,
    pub status:      String, // "pending" | "running" | "done" | "error"
    pub duration_ms: Option<u64>,
    pub output:      Option<serde_json::Value>,
    pub error:       Option<String>,
}
