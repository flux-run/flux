/// Fluxbase Tool Infrastructure
///
/// Single execution layer for all tool invocations:
///   Function → tool_executor.run()
///   Workflow → tool_executor.run()   (Phase 2)
///   Agent    → tool_executor.run()   (Phase 3)
///
/// Composio powers the underlying 900+ app integrations.
/// Users see only "fluxbase tools" — Composio is an internal implementation detail.

pub mod registry;
pub mod executor;
pub mod composio;

#[allow(unused_imports)]
pub use executor::ToolExecutor;
#[allow(unused_imports)]
pub use registry::{ToolRegistry, ToolMeta};
