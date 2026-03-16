use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::deno_runtime::{ExecutionMode, FetchCheckpoint, JsExecutionOutput, JsIsolate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub execution_id: String,
    pub request_id: String,
    pub code_version: String,
    pub mode: ExecutionMode,
}

impl ExecutionContext {
    pub fn new(code_version: impl Into<String>) -> Self {
        Self {
            execution_id: Uuid::new_v4().to_string(),
            request_id: Uuid::new_v4().to_string(),
            code_version: code_version.into(),
            mode: ExecutionMode::Live,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub execution_id: String,
    pub request_id: String,
    pub code_version: String,
    pub status: String,
    pub body: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: i32,
    pub checkpoints: Vec<FetchCheckpoint>,
}

#[derive(Debug)]
pub struct IsolatePool {
    workers: Vec<IsolateWorker>,
    next: AtomicUsize,
    queue_send_timeout: Duration,
    result_timeout: Duration,
}

#[derive(Debug)]
struct IsolateWorker {
    sender: mpsc::Sender<WorkItem>,
}

struct WorkItem {
    payload: serde_json::Value,
    context: ExecutionContext,
    recorded_checkpoints: Vec<FetchCheckpoint>,
    result_tx: oneshot::Sender<ExecutionResult>,
}

impl IsolatePool {
    pub fn new(size: usize, user_code: &str) -> Result<Self> {
        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(IsolateWorker {
                sender: spawn_isolate_worker(id, user_code.to_string())?,
            });
        }

        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
            queue_send_timeout: Duration::from_secs(30),
            result_timeout: Duration::from_secs(120),
        })
    }

    pub async fn execute(&self, payload: serde_json::Value, context: ExecutionContext) -> ExecutionResult {
        self.execute_with_recorded(payload, context, Vec::new()).await
    }

    /// Execute user code with pre-recorded checkpoints for replay.
    pub async fn execute_with_recorded(
        &self,
        payload: serde_json::Value,
        context: ExecutionContext,
        recorded_checkpoints: Vec<FetchCheckpoint>,
    ) -> ExecutionResult {
        if self.workers.is_empty() {
            return error_result(context, "isolate pool is empty");
        }

        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let worker = &self.workers[idx];

        let (result_tx, result_rx) = oneshot::channel::<ExecutionResult>();
        let work = WorkItem {
            payload,
            context: context.clone(),
            recorded_checkpoints,
            result_tx,
        };

        match tokio::time::timeout(self.queue_send_timeout, worker.sender.send(work)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return error_result(context, "isolate worker is unavailable"),
            Err(_) => return error_result(context, "timed out while waiting for isolate queue capacity"),
        }

        match tokio::time::timeout(self.result_timeout, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => error_result(context, "isolate worker dropped execution result"),
            Err(_) => error_result(context, "timed out while waiting for isolate execution result"),
        }
    }
}

fn spawn_isolate_worker(
    isolate_id: usize,
    user_code: String,
) -> Result<mpsc::Sender<WorkItem>> {
    let (tx, mut rx) = mpsc::channel::<WorkItem>(32);
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
                        Some(isolate)
                    }
                    Err(err) => {
                        let _ = init_tx.send(Err(err.to_string()));
                        return;
                    }
                };

                while let Some(work) = rx.recv().await {
                    let iso = match isolate.as_mut() {
                        Some(iso) => iso,
                        None => {
                            let _ = work.result_tx.send(error_result(work.context, "isolate unavailable after failed re-creation"));
                            continue;
                        }
                    };
                    let context = work.context.clone();
                    let started = std::time::Instant::now();
                    let result = match iso.execute_with_recorded(work.payload, work.context, work.recorded_checkpoints).await {
                        Ok(JsExecutionOutput {
                            output,
                            checkpoints,
                            error,
                        }) => {
                            let (status, body, error) = match error {
                                Some(err) => ("error".to_string(), serde_json::Value::Null, Some(err)),
                                None => (
                                    "ok".to_string(),
                                    serde_json::json!({
                                        "isolate_id": isolate_id,
                                        "output": output,
                                    }),
                                    None,
                                ),
                            };

                            ExecutionResult {
                                execution_id: context.execution_id,
                                request_id: context.request_id,
                                code_version: context.code_version,
                                status,
                                body,
                                error,
                                duration_ms: started.elapsed().as_millis() as i32,
                                checkpoints,
                            }
                        }
                        Err(err) => ExecutionResult {
                            execution_id: context.execution_id,
                            request_id: context.request_id,
                            code_version: context.code_version,
                            status: "error".to_string(),
                            body: serde_json::Value::Null,
                            error: Some(err.to_string()),
                            duration_ms: started.elapsed().as_millis() as i32,
                            checkpoints: vec![],
                        },
                    };

                    let _ = work.result_tx.send(result);

                    // Drop the old V8 isolate BEFORE creating a new one.
                    // V8 requires isolates to be dropped in reverse creation order.
                    drop(isolate.take());
                    isolate = match JsIsolate::new(&user_code, isolate_id) {
                        Ok(fresh) => Some(fresh),
                        Err(err) => {
                            tracing::error!(%isolate_id, %err, "failed to re-create isolate; worker exiting");
                            break;
                        }
                    };
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

fn error_result(context: ExecutionContext, message: impl Into<String>) -> ExecutionResult {
    ExecutionResult {
        execution_id: context.execution_id,
        request_id: context.request_id,
        code_version: context.code_version,
        status: "error".to_string(),
        body: serde_json::Value::Null,
        error: Some(message.into()),
        duration_ms: 0,
        checkpoints: vec![],
    }
}
