//! In-process implementation of [`AgentDispatch`].
//!
//! Calls `agent::run()` directly — no HTTP, no serialisation overhead.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::AgentDispatch;

pub struct InProcessAgentDispatch {
    pub state: Arc<agent::AgentState>,
}

#[async_trait]
impl AgentDispatch for InProcessAgentDispatch {
    async fn run(
        &self,
        name:       &str,
        input:      Value,
        request_id: &str,
        project_id: Uuid,
        secrets:    HashMap<String, String>,
    ) -> Result<Value, String> {
        agent::run(&self.state, name, input, request_id, project_id, &secrets)
            .await
            .map_err(|e| e.to_string())
    }
}
