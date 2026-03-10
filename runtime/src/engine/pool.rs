use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};

use super::executor::ExecutionResult;

/// A task sent to an isolate worker.
struct ExecutionTask {
    code:       String,
    secrets:    HashMap<String, String>,
    payload:    serde_json::Value,
    tenant_id:  String,
    tenant_slug: String,
    reply:      oneshot::Sender<Result<ExecutionResult, String>>,
}

/// A fixed pool of OS threads, each owning a reusable Tokio runtime.
///
/// When a function invocation arrives, a task is dispatched to a free worker via
/// an async mpsc channel.  The worker creates a fresh `JsRuntime` for the task
/// (clean state isolation between functions) and drives its event loop to
/// completion, then sends the result back via a oneshot channel.
///
/// # Why this matters
///
/// Without pooling every invocation spawns an OS thread (`~0.5 ms` + 8 MB stack)
/// and its own Tokio runtime (`~1 ms`).  Under concurrent load this creates an
/// unbounded number of threads, which is the primary cause of memory pressure
/// when you have many simultaneous executions.
///
/// With pooling:
/// - Thread count is capped at `workers` (default: 2× available CPUs).
/// - Excess requests queue in the channel backpressure buffer instead of spawning.
/// - Thread-spawn + runtime-init cost is paid once at startup, not per request.
///
/// Memory is now bounded: `workers × (V8 isolate RSS ~5 MB + stack 8 MB)` regardless
/// of how many functions are deployed.
#[derive(Clone)]
pub struct IsolatePool {
    sender:  mpsc::Sender<ExecutionTask>,
    workers: usize,
}

impl IsolatePool {
    /// Spawn `workers` OS threads and return a pool ready to accept executions.
    ///
    /// The channel buffer is `workers * 4` so bursts can queue without back-pressure
    /// until the burst is 4× the worker count.
    pub fn new(workers: usize) -> Self {
        let workers = workers.max(1);
        let (tx, rx) = mpsc::channel::<ExecutionTask>(workers * 4);

        // Workers share a single receiver protected by a mutex.
        // Each worker races to pull the next task off the queue.
        let rx = std::sync::Arc::new(tokio::sync::Mutex::new(rx));

        for id in 0..workers {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("isolate-worker-{}", id))
                .stack_size(8 * 1024 * 1024)  // explicit 8 MB stack
                .spawn(move || {
                    // Each worker owns its own single-threaded Tokio runtime.
                    // This is intentional: Deno's JsRuntime is !Send and must
                    // stay on the thread that created it.
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("isolate worker tokio runtime");

                    rt.block_on(async move {
                        loop {
                            let task = {
                                let mut guard = rx.lock().await;
                                guard.recv().await
                            };
                            match task {
                                None => {
                                    tracing::info!(worker = id, "isolate channel closed, shutting down");
                                    break;
                                }
                                Some(t) => {
                                    let result = run_in_isolate(
                                        t.code, t.secrets, t.payload,
                                        t.tenant_id, t.tenant_slug,
                                    ).await;
                                    // If the caller dropped the oneshot (timeout), discard silently.
                                    let _ = t.reply.send(result);
                                }
                            }
                        }
                    });
                })
                .expect("failed to spawn isolate worker thread");
        }

        Self { sender: tx, workers }
    }

    /// Spawn a pool sized to 2× logical CPUs (min 2, max 16).
    pub fn default_sized() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = (cpus * 2).clamp(2, 16);
        tracing::info!(workers, "isolate pool started");
        Self::new(workers)
    }

    pub fn workers(&self) -> usize { self.workers }

    /// Dispatch a function execution to the next available worker.
    ///
    /// Returns an error if the pool is shut down or if the per-invocation timeout
    /// (11 s) fires before a worker accepts the task.
    pub async fn execute(
        &self,
        code:        String,
        secrets:     HashMap<String, String>,
        payload:     serde_json::Value,
        tenant_id:   String,
        tenant_slug: String,
    ) -> Result<ExecutionResult, String> {
        let (reply_tx, reply_rx) = oneshot::channel();

        let task = ExecutionTask {
            code, secrets, payload, tenant_id, tenant_slug,
            reply: reply_tx,
        };

        // Sending is async — if all workers are busy the caller awaits here
        // (backpressure) rather than spawning unbounded threads.
        self.sender.send(task).await
            .map_err(|_| "isolate pool is shut down".to_string())?;

        // Wait for the worker's reply with a hard outer timeout.
        timeout(Duration::from_secs(11), reply_rx)
            .await
            .map_err(|_| "isolate pool: invocation timed out waiting for worker".to_string())?
            .map_err(|_| "isolate pool: worker dropped reply channel".to_string())?
    }
}

/// Execute a function with the full FluxContext (workflow, tools, agent).
/// Delegates to the main executor which registers all Deno ops and provides
/// the complete __ctx implementation.
async fn run_in_isolate(
    code:        String,
    secrets:     HashMap<String, String>,
    payload:     serde_json::Value,
    tenant_id:   String,
    tenant_slug: String,
) -> Result<ExecutionResult, String> {
    super::executor::execute_function(code, secrets, payload, tenant_id, tenant_slug).await
}
