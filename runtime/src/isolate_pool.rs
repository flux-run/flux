use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub request_id: String,
    pub code_version: String,
}

impl ExecutionContext {
    pub fn new(code_version: impl Into<String>) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            code_version: code_version.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub request_id: String,
    pub code_version: String,
    pub status: String,
    pub body: serde_json::Value,
}

#[derive(Debug)]
pub struct IsolatePool {
    semaphore: Arc<Semaphore>,
    available_ids: Arc<Mutex<Vec<usize>>>,
}

impl IsolatePool {
    pub fn new(size: usize) -> Self {
        let mut ids = Vec::with_capacity(size);
        for id in 0..size {
            ids.push(id);
        }

        Self {
            semaphore: Arc::new(Semaphore::new(size)),
            available_ids: Arc::new(Mutex::new(ids)),
        }
    }

    pub async fn acquire(self: &Arc<Self>) -> Result<PooledIsolate> {
        let permit = self.semaphore.clone().acquire_owned().await?;

        let isolate_id = {
            let mut ids = self
                .available_ids
                .lock()
                .map_err(|_| anyhow::anyhow!("isolate id pool poisoned"))?;
            ids.pop().ok_or_else(|| anyhow::anyhow!("no isolate id available"))?
        };

        Ok(PooledIsolate {
            isolate_id,
            pool: self.clone(),
            permit,
            context: None,
        })
    }
}

pub struct PooledIsolate {
    isolate_id: usize,
    pool: Arc<IsolatePool>,
    permit: OwnedSemaphorePermit,
    context: Option<ExecutionContext>,
}

impl PooledIsolate {
    pub fn set_context(&mut self, context: ExecutionContext) {
        self.context = Some(context);
    }

    pub async fn run(&self, payload: serde_json::Value, route: &str) -> ExecutionResult {
        tokio::task::yield_now().await;

        let context = self.context.clone().unwrap_or_else(|| ExecutionContext {
            request_id: Uuid::new_v4().to_string(),
            code_version: "unknown".to_string(),
        });

        ExecutionResult {
            request_id: context.request_id,
            code_version: context.code_version,
            status: "ok".to_string(),
            body: serde_json::json!({
                "ok": true,
                "route": route,
                "isolate_id": self.isolate_id,
                "payload": payload,
            }),
        }
    }
}

impl Drop for PooledIsolate {
    fn drop(&mut self) {
        let _ = &self.permit;
        if let Ok(mut ids) = self.pool.available_ids.lock() {
            ids.push(self.isolate_id);
        }
    }
}
