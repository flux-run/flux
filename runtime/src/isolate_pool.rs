use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use uuid::Uuid;

use crate::deno_runtime::{ExecutionMode, JsIsolate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub request_id: String,
    pub code_version: String,
    pub mode: ExecutionMode,
}

impl ExecutionContext {
    pub fn new(code_version: impl Into<String>) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            code_version: code_version.into(),
            mode: ExecutionMode::Live,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub request_id: String,
    pub code_version: String,
    pub status: String,
    pub body: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct IsolatePool {
    semaphore: Arc<Semaphore>,
    available_ids: Arc<Mutex<Vec<usize>>>,
    workers: Vec<mpsc::UnboundedSender<WorkerCommand>>,
}

impl IsolatePool {
    pub fn new(size: usize, user_code: &str) -> Result<Self> {
        let mut ids = Vec::with_capacity(size);
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            ids.push(id);
            workers.push(spawn_isolate_worker(id, user_code.to_string())?);
        }

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(size)),
            available_ids: Arc::new(Mutex::new(ids)),
            workers,
        })
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
            worker: self.workers[isolate_id].clone(),
            pool: self.clone(),
            permit,
            context: None,
        })
    }
}

pub struct PooledIsolate {
    isolate_id: usize,
    worker: mpsc::UnboundedSender<WorkerCommand>,
    pool: Arc<IsolatePool>,
    permit: OwnedSemaphorePermit,
    context: Option<ExecutionContext>,
}

impl PooledIsolate {
    pub fn set_context(&mut self, context: ExecutionContext) {
        self.context = Some(context);
    }

    pub async fn run(&self, payload: serde_json::Value, _route: &str) -> ExecutionResult {
        tokio::task::yield_now().await;

        let context = self.context.clone().unwrap_or_else(|| ExecutionContext {
            request_id: Uuid::new_v4().to_string(),
            code_version: "unknown".to_string(),
            mode: ExecutionMode::Live,
        });

        let (tx, rx) = oneshot::channel();
        let sent = self.worker.send(WorkerCommand {
            payload,
            context: context.clone(),
            reply: tx,
        });

        if sent.is_err() {
            return ExecutionResult {
                request_id: context.request_id,
                code_version: context.code_version,
                status: "error".to_string(),
                body: serde_json::Value::Null,
                error: Some("isolate worker is unavailable".to_string()),
            };
        }

        match rx.await {
            Ok(Ok(body)) => ExecutionResult {
                request_id: context.request_id,
                code_version: context.code_version,
                status: "ok".to_string(),
                body: serde_json::json!({
                    "isolate_id": self.isolate_id,
                    "output": body,
                }),
                error: None,
            },
            Ok(Err(err)) => ExecutionResult {
                request_id: context.request_id,
                code_version: context.code_version,
                status: "error".to_string(),
                body: serde_json::Value::Null,
                error: Some(err),
            },
            Err(err) => ExecutionResult {
                request_id: context.request_id,
                code_version: context.code_version,
                status: "error".to_string(),
                body: serde_json::Value::Null,
                error: Some(format!("isolate worker dropped response: {err}")),
            },
        }
    }
}

struct WorkerCommand {
    payload: serde_json::Value,
    context: ExecutionContext,
    reply: oneshot::Sender<std::result::Result<serde_json::Value, String>>,
}

fn spawn_isolate_worker(
    isolate_id: usize,
    user_code: String,
) -> Result<mpsc::UnboundedSender<WorkerCommand>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WorkerCommand>();
    let (init_tx, init_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    std::thread::Builder::new()
        .name(format!("flux-isolate-{}", isolate_id))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    let _ = init_tx.send(Err(format!("failed to create tokio runtime: {err}")));
                    return;
                }
            };

            runtime.block_on(async move {
                let mut isolate = match JsIsolate::new(&user_code, isolate_id) {
                    Ok(isolate) => {
                        let _ = init_tx.send(Ok(()));
                        isolate
                    }
                    Err(err) => {
                        let _ = init_tx.send(Err(err.to_string()));
                        return;
                    }
                };

                while let Some(command) = rx.recv().await {
                    let result = isolate
                        .execute(command.payload, command.context)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = command.reply.send(result);
                }
            });
        })
        .map_err(|err| anyhow::anyhow!("failed to spawn isolate worker thread: {err}"))?;

    match init_rx.recv() {
        Ok(Ok(())) => Ok(tx),
        Ok(Err(err)) => Err(anyhow::anyhow!("failed to initialize isolate worker: {err}")),
        Err(err) => Err(anyhow::anyhow!(
            "failed to receive isolate worker initialization status: {err}"
        )),
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
