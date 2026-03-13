//! In-process implementation of [`RuntimeDispatch`].
//!
//! Calls the runtime execution engine directly — no HTTP, no serialization.

use std::sync::Arc;

use async_trait::async_trait;

use job_contract::dispatch::{ExecuteRequest, ExecuteResponse, RuntimeDispatch};
use runtime::AppState as RuntimeState;

/// Calls the runtime crate's execution service directly.
pub struct InProcessRuntimeDispatch {
    pub state: Arc<RuntimeState>,
}

#[async_trait]
impl RuntimeDispatch for InProcessRuntimeDispatch {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse, String> {
        runtime::execute::service::invoke(Arc::clone(&self.state), req).await
    }
}
