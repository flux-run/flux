use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use deno_core::{JsRuntime, RuntimeOptions};

use super::executor::{ExecutionResult, LogLine};

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

/// Create a fresh `JsRuntime`, compile and run the function wrapper, and return
/// the structured result.  Always called inside a dedicated worker thread that
/// owns its own Tokio runtime.
async fn run_in_isolate(
    code:        String,
    secrets:     HashMap<String, String>,
    payload:     serde_json::Value,
    tenant_id:   String,
    tenant_slug: String,
) -> Result<ExecutionResult, String> {
    // A fresh isolate per invocation gives clean global state — no function can
    // pollute the next.  The thread itself is reused (avoiding the expensive
    // thread-spawn + tokio-runtime-init that the old per-invocation model paid).
    let mut rt = JsRuntime::new(RuntimeOptions {
        ..Default::default()
    });

    let secrets_json = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
    let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

    let wrapper = format!(r#"
        var __fluxbase_fn;

        (async () => {{
            const __fluxbase_logs = [];

            const __secrets = {secrets_json};
            const __payload = {payload_json};

            const __ctx = {{
                tenant: {{
                    id: "{tenant_id}",
                    slug: "{tenant_slug}",
                }},
                payload: __payload,
                env: __secrets,
                secrets: {{
                    get: (key) => __secrets[key] !== undefined ? __secrets[key] : null,
                }},
                log: (message, level) => {{
                    __fluxbase_logs.push({{
                        level: level || "info",
                        message: String(message),
                    }});
                }},
            }};

            {code}

            let __result;
            let target_fn = __fluxbase_fn;

            if (target_fn && target_fn.default) {{
                target_fn = target_fn.default;
            }}

            if (typeof target_fn === 'object' && target_fn !== null && target_fn.__fluxbase === true) {{
                try {{
                    __fluxbase_logs.push({{ level: "debug", message: "Calling target_fn.execute()" }});
                    __result = await target_fn.execute(__payload, __ctx);
                    __fluxbase_logs.push({{ level: "debug", message: "target_fn.execute() returned smoothly" }});
                }} catch (e) {{
                    const code = e.code || 'EXECUTION_ERROR';
                    throw new Error(JSON.stringify({{ code, message: e.message }}));
                }}
            }} else if (typeof target_fn === 'function') {{
                __fluxbase_logs.push({{ level: "debug", message: "Calling raw function" }});
                __result = await target_fn(__ctx);
            }} else {{
                throw new Error("Bundle must export a defineFunction() result or an async function. target_fn=" + typeof target_fn);
            }}

            __fluxbase_logs.push({{ level: "debug", message: "Returning result envelope" }});
            return {{ result: __result, logs: __fluxbase_logs }};
        }})()
    "#);

    let res = timeout(Duration::from_secs(10), async {
        let res = rt.execute_script("<anon>", wrapper)
            .map_err(|e| format!("Execution error: {}", e))?;

        let resolved_future = rt.resolve(res);
        let resolved = rt.with_event_loop_promise(resolved_future, Default::default()).await
            .map_err(|e| format!("Promise resolution error: {}", e))?;

        let mut scope = rt.handle_scope();
        let local = deno_core::v8::Local::new(&mut scope, resolved);
        let json_val = deno_core::serde_v8::from_v8::<serde_json::Value>(&mut scope, local)
            .map_err(|e| format!("Serialization error: {}", e))?;

        Ok::<serde_json::Value, String>(json_val)
    }).await;

    match res {
        Ok(Ok(val)) => {
            let output = val.get("result").cloned().unwrap_or(val.clone());
            let logs: Vec<LogLine> = val.get("logs")
                .and_then(|l| serde_json::from_value(l.clone()).ok())
                .unwrap_or_default();
            Ok(ExecutionResult { output, logs })
        }
        Ok(Err(e)) => Err(e),
        Err(_)     => Err("Function execution timed out after 10 seconds".to_string()),
    }
}
