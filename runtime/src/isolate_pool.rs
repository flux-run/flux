use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::artifact::RuntimeArtifact;
use crate::deno_runtime::{
    ExecutionMode, FetchCheckpoint, JsExecutionOutput, JsIsolate, LogEntry, NetRequest,
    NetRequestExecution,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub execution_id: String,
    pub request_id: String,
    pub project_id: Option<String>,
    pub code_version: String,
    pub mode: ExecutionMode,
}

impl ExecutionContext {
    pub fn new(code_version: impl Into<String>) -> Self {
        Self {
            execution_id: Uuid::new_v4().to_string(),
            request_id: Uuid::new_v4().to_string(),
            project_id: None,
            code_version: code_version.into(),
            mode: ExecutionMode::Live,
        }
    }

    pub fn with_project(code_version: impl Into<String>, project_id: Option<String>) -> Self {
        Self {
            execution_id: Uuid::new_v4().to_string(),
            request_id: Uuid::new_v4().to_string(),
            project_id,
            code_version: code_version.into(),
            mode: ExecutionMode::Live,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub execution_id: String,
    pub request_id: String,
    pub project_id: Option<String>,
    pub code_version: String,
    pub status: String,
    pub body: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: i32,
    pub checkpoints: Vec<FetchCheckpoint>,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug)]
pub struct IsolatePool {
    workers: Vec<IsolateWorker>,
    next: AtomicUsize,
    queue_send_timeout: Duration,
    result_timeout: Duration,
    /// True when all isolates in this pool run a `Deno.serve()` server app
    /// instead of a one-shot exported handler.
    pub is_server_mode: bool,
}

#[derive(Debug)]
struct IsolateWorker {
    sender: mpsc::Sender<WorkItem>,
}

struct WorkItem {
    payload: serde_json::Value,
    context: ExecutionContext,
    recorded_checkpoints: Vec<FetchCheckpoint>,
    /// Some(_) means this is a server-mode HTTP dispatch, not a handler invocation.
    net_request: Option<NetRequest>,
    result_tx: oneshot::Sender<ExecutionResult>,
}

impl IsolatePool {
    pub fn new(size: usize, artifact: RuntimeArtifact) -> Result<Self> {
        let mut workers = Vec::with_capacity(size);
        let mut is_server_mode = false;
        for id in 0..size {
            let (sender, server_mode) = spawn_isolate_worker(id, artifact.clone())?;
            if id == 0 {
                is_server_mode = server_mode;
            }
            workers.push(IsolateWorker { sender });
        }

        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
            queue_send_timeout: Duration::from_secs(30),
            result_timeout: Duration::from_secs(120),
            is_server_mode,
        })
    }

    pub fn new_with_mode(
        size: usize,
        artifact: RuntimeArtifact,
        is_server_mode: bool,
    ) -> Result<Self> {
        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            let sender = spawn_isolate_worker_with_mode(id, artifact.clone(), is_server_mode)?;
            workers.push(IsolateWorker { sender });
        }

        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
            queue_send_timeout: Duration::from_secs(30),
            result_timeout: Duration::from_secs(120),
            is_server_mode,
        })
    }

    pub async fn execute(
        &self,
        payload: serde_json::Value,
        context: ExecutionContext,
    ) -> ExecutionResult {
        self.execute_with_recorded(payload, context, Vec::new())
            .await
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
            net_request: None,
            result_tx,
        };

        match tokio::time::timeout(self.queue_send_timeout, worker.sender.send(work)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return error_result(context, "isolate worker is unavailable"),
            Err(_) => {
                return error_result(
                    context,
                    "timed out while waiting for isolate queue capacity",
                );
            }
        }

        match tokio::time::timeout(self.result_timeout, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => error_result(context, "isolate worker dropped execution result"),
            Err(_) => error_result(
                context,
                "timed out while waiting for isolate execution result",
            ),
        }
    }

    /// Dispatch a single HTTP request into a server-mode isolate pool.
    /// The isolate's `Deno.serve` handler produces the response.
    pub async fn execute_net_request(
        &self,
        context: ExecutionContext,
        net_request: NetRequest,
    ) -> ExecutionResult {
        self.execute_net_request_with_recorded(context, net_request, Vec::new())
            .await
    }

    pub async fn execute_net_request_with_recorded(
        &self,
        context: ExecutionContext,
        net_request: NetRequest,
        recorded_checkpoints: Vec<FetchCheckpoint>,
    ) -> ExecutionResult {
        if self.workers.is_empty() {
            return error_result(context, "isolate pool is empty");
        }

        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let worker = &self.workers[idx];

        let (result_tx, result_rx) = oneshot::channel::<ExecutionResult>();
        let work = WorkItem {
            payload: serde_json::Value::Null,
            context: context.clone(),
            recorded_checkpoints,
            net_request: Some(net_request),
            result_tx,
        };

        match tokio::time::timeout(self.queue_send_timeout, worker.sender.send(work)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return error_result(context, "isolate worker is unavailable"),
            Err(_) => {
                return error_result(
                    context,
                    "timed out while waiting for isolate queue capacity",
                );
            }
        }

        match tokio::time::timeout(self.result_timeout, result_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => error_result(context, "isolate worker dropped execution result"),
            Err(_) => error_result(
                context,
                "timed out while waiting for isolate execution result",
            ),
        }
    }
}

fn spawn_isolate_worker(
    isolate_id: usize,
    artifact: RuntimeArtifact,
) -> Result<(mpsc::Sender<WorkItem>, bool)> {
    let (tx, mut rx) = mpsc::channel::<WorkItem>(64);
    let (init_tx, init_rx) = std::sync::mpsc::channel::<std::result::Result<bool, String>>();

    std::thread::Builder::new()
        .name(format!("flux-isolate-{}", isolate_id))
        .spawn(move || {
            // Each isolate worker thread owns its own single-threaded tokio runtime
            // AND a LocalSet so that `spawn_local` futures can run alongside the
            // V8 event loop on the same thread.
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

            let local_set = tokio::task::LocalSet::new();

            local_set.block_on(&runtime, async move {
                let isolate_result = match &artifact {
                    RuntimeArtifact::Inline(artifact) => JsIsolate::new(&artifact.code, isolate_id).await,
                    RuntimeArtifact::Built(artifact) => {
                        JsIsolate::new_from_artifact(artifact).await
                    }
                };

                let is_server_mode = match isolate_result {
                    Ok(iso) => {
                        let detected = iso.is_server_mode;
                        let _ = init_tx.send(Ok(detected));
                        detected
                    }
                    Err(err) => {
                        let _ = init_tx.send(Err(format!("{:#}", err)));
                        return;
                    }
                };

                // Process work items serially per worker. Each HTTP execution gets
                // a fresh isolate so requests reuse the program artifact, not the
                // previous request's JS heap.
                while let Some(work) = rx.recv().await {
                    let context = work.context.clone();
                    let started = std::time::Instant::now();

                    let isolate_result = match &artifact {
                        RuntimeArtifact::Inline(artifact) => {
                            JsIsolate::new(&artifact.code, isolate_id).await
                        }
                        RuntimeArtifact::Built(artifact) => {
                            JsIsolate::new_from_artifact(artifact).await
                        }
                    };

                    let mut isolate = match isolate_result {
                        Ok(iso) => iso,
                        Err(err) => {
                            let _ = work.result_tx.send(error_result(
                                work.context,
                                format!("failed to initialize isolate: {err:#}"),
                            ));
                            continue;
                        }
                    };

                    let result = if is_server_mode {
                        match work.net_request {
                            Some(net_req) => match isolate
                                .dispatch_request_with_recorded(
                                    work.context.clone(),
                                    net_req,
                                    work.recorded_checkpoints,
                                )
                                .await
                            {
                                Ok(NetRequestExecution {
                                    response: net_resp,
                                    checkpoints,
                                    logs,
                                }) => ExecutionResult {
                                    execution_id: context.execution_id, project_id: context.project_id.clone(),
                                    request_id: context.request_id,
                                    code_version: context.code_version,
                                    status: "ok".to_string(),
                                    body: serde_json::json!({
                                        "net_response": {
                                            "status": net_resp.status,
                                            "headers": net_resp.headers,
                                            "body": net_resp.body,
                                        }
                                    }),
                                    error: None,
                                    duration_ms: started.elapsed().as_millis() as i32,
                                    checkpoints,
                                    logs,
                                },
                                Err(err) => error_result(work.context, err.to_string()),
                            },
                            None => error_result(
                                work.context,
                                "server-mode isolate received non-HTTP work item",
                            ),
                        }
                    } else {
                        match isolate
                            .execute_with_recorded(
                                work.payload,
                                work.context,
                                work.recorded_checkpoints,
                            )
                            .await
                        {
                            Ok(JsExecutionOutput {
                                output,
                                checkpoints,
                                error,
                                logs,
                            }) => {
                                let (status, body, error) = match error {
                                    Some(err) => {
                                        ("error".to_string(), serde_json::Value::Null, Some(err))
                                    }
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
                                    execution_id: context.execution_id, project_id: context.project_id.clone(),
                                    request_id: context.request_id,
                                    code_version: context.code_version,
                                    status,
                                    body,
                                    error,
                                    duration_ms: started.elapsed().as_millis() as i32,
                                    checkpoints,
                                    logs,
                                }
                            }
                            Err(err) => ExecutionResult {
                                execution_id: context.execution_id,
                                project_id: context.project_id.clone(),
                                request_id: context.request_id,
                                code_version: context.code_version,
                                status: "error".to_string(),
                                body: serde_json::Value::Null,
                                error: Some(err.to_string()),
                                duration_ms: started.elapsed().as_millis() as i32,
                                checkpoints: vec![],
                                logs: vec![],
                            },
                        }
                    };

                    let _ = work.result_tx.send(result);
                }
            });
        })
        .map_err(|err| anyhow::anyhow!("failed to spawn isolate worker thread: {err}"))?;

    match init_rx.recv() {
        Ok(Ok(is_server_mode)) => Ok((tx, is_server_mode)),
        Ok(Err(err)) => Err(anyhow::anyhow!(
            "failed to initialize isolate worker: {err}"
        )),
        Err(err) => Err(anyhow::anyhow!(
            "failed to receive isolate worker initialization status: {err}"
        )),
    }
}

fn spawn_isolate_worker_with_mode(
    isolate_id: usize,
    artifact: RuntimeArtifact,
    is_server_mode: bool,
) -> Result<mpsc::Sender<WorkItem>> {
    let (tx, mut rx) = mpsc::channel::<WorkItem>(64);

    std::thread::Builder::new()
        .name(format!("flux-isolate-{}", isolate_id))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => {
                    return;
                }
            };

            let local_set = tokio::task::LocalSet::new();

            local_set.block_on(&runtime, async move {
                while let Some(work) = rx.recv().await {
                    let context = work.context.clone();
                    let started = std::time::Instant::now();

                    let isolate_result = match &artifact {
                        RuntimeArtifact::Inline(artifact) => {
                            JsIsolate::new(&artifact.code, isolate_id).await
                        }
                        RuntimeArtifact::Built(artifact) => {
                            JsIsolate::new_from_artifact(artifact).await
                        }
                    };

                    let mut isolate = match isolate_result {
                        Ok(iso) => iso,
                        Err(err) => {
                            let _ = work.result_tx.send(error_result(
                                work.context,
                                format!("failed to initialize isolate: {err:#}"),
                            ));
                            continue;
                        }
                    };

                    let result = if is_server_mode {
                        match work.net_request {
                            Some(net_req) => match isolate
                                .dispatch_request_with_recorded(
                                    work.context.clone(),
                                    net_req,
                                    work.recorded_checkpoints,
                                )
                                .await
                            {
                                Ok(NetRequestExecution {
                                    response: net_resp,
                                    checkpoints,
                                    logs,
                                }) => ExecutionResult {
                                    execution_id: context.execution_id, project_id: context.project_id.clone(),
                                    request_id: context.request_id,
                                    code_version: context.code_version,
                                    status: "ok".to_string(),
                                    body: serde_json::json!({
                                        "net_response": {
                                            "status": net_resp.status,
                                            "headers": net_resp.headers,
                                            "body": net_resp.body,
                                        }
                                    }),
                                    error: None,
                                    duration_ms: started.elapsed().as_millis() as i32,
                                    checkpoints,
                                    logs,
                                },
                                Err(err) => error_result(work.context, err.to_string()),
                            },
                            None => error_result(
                                work.context,
                                "server-mode isolate received non-HTTP work item",
                            ),
                        }
                    } else {
                        match isolate
                            .execute_with_recorded(
                                work.payload,
                                work.context,
                                work.recorded_checkpoints,
                            )
                            .await
                        {
                            Ok(JsExecutionOutput {
                                output,
                                checkpoints,
                                error,
                                logs,
                            }) => {
                                let (status, body, error) = match error {
                                    Some(err) => {
                                        ("error".to_string(), serde_json::Value::Null, Some(err))
                                    }
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
                                    execution_id: context.execution_id, project_id: context.project_id.clone(),
                                    request_id: context.request_id,
                                    code_version: context.code_version,
                                    status,
                                    body,
                                    error,
                                    duration_ms: started.elapsed().as_millis() as i32,
                                    checkpoints,
                                    logs,
                                }
                            }
                            Err(err) => ExecutionResult {
                                execution_id: context.execution_id,
                                request_id: context.request_id,
                                project_id: context.project_id.clone(),
                                code_version: context.code_version,
                                status: "error".to_string(),
                                body: serde_json::Value::Null,
                                error: Some(err.to_string()),
                                duration_ms: started.elapsed().as_millis() as i32,
                                checkpoints: vec![],
                                logs: vec![],
                            },
                        }
                    };

                    let _ = work.result_tx.send(result);
                }
            });
        })
        .map_err(|err| anyhow::anyhow!("failed to spawn isolate worker thread: {err}"))?;

    Ok(tx)
}

fn error_result(context: ExecutionContext, message: impl Into<String>) -> ExecutionResult {
    ExecutionResult {
        execution_id: context.execution_id,
        request_id: context.request_id,
        project_id: context.project_id.clone(),
        code_version: context.code_version,
        status: "error".to_string(),
        body: serde_json::Value::Null,
        error: Some(message.into()),
        duration_ms: 0,
        checkpoints: vec![],
        logs: vec![],
    }
}
