//! Warm isolate pool — concurrent multi-task V8 execution.
//!
//! Each worker thread runs a persistent JS bootstrap loop that accepts tasks via
//! `op_next_task`. Multiple requests can be in-flight concurrently within a single
//! V8 isolate: when Task A suspends on `await op_queue_push(...)`, V8 drives Task B.
//!
//! Pool dispatch uses round-robin across workers. Results return via per-request
//! oneshot channels registered in each worker's ResultRegistry.
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use super::executor::{
    create_concurrent_js_runtime, DbContext, ExecutionResult, QueueContext,
    ResultRegistry, SharedTaskReceiver,
};

/// Handle to a single concurrent worker.
struct WorkerHandle {
    /// Send tasks to this worker's op_next_task channel.
    task_tx:   mpsc::Sender<serde_json::Value>,
    /// Per-request reply channels; shared with the worker's OpState.
    registry:  ResultRegistry,
    /// Number of requests currently in-flight on this worker.
    in_flight: Arc<AtomicUsize>,
    /// Bundle key of the last function dispatched to this worker.
    /// Used for bundle-key affinity routing: requests for the same function
    /// (same code bundle) are routed to the same worker when possible, so
    /// the V8 isolate has the module already evaluated in its heap.
    bundle_key: Arc<std::sync::Mutex<Option<String>>>,
}

/// Pool of V8 isolate workers, each running a concurrent bootstrap loop.
#[derive(Clone)]
pub struct IsolatePool {
    workers:      Vec<Arc<WorkerHandle>>,
    next_worker:  Arc<AtomicUsize>,
    timeout_secs: u64,
    worker_count: usize,
}

impl IsolatePool {
    /// Spawn `workers` OS threads and return a pool ready to accept executions.
    pub fn new(workers: usize, timeout_secs: u64) -> Self {
        let workers = workers.max(1);
        let handles: Vec<Arc<WorkerHandle>> = (0..workers)
            .map(|id| {
                let (task_tx, task_rx) = mpsc::channel::<serde_json::Value>(256);
                let registry: ResultRegistry =
                    Arc::new(std::sync::Mutex::new(HashMap::new()));
                let task_receiver: SharedTaskReceiver =
                    Arc::new(tokio::sync::Mutex::new(task_rx));

                let registry_clone = registry.clone();

                std::thread::Builder::new()
                    .name(format!("isolate-worker-{}", id))
                    .stack_size(8 * 1024 * 1024)
                    .spawn(move || {
                        let tokio_rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("isolate worker tokio runtime");

                        tokio_rt.block_on(async move {
                            let mut rt = create_concurrent_js_runtime(task_receiver, registry_clone);
                            tracing::debug!(worker = id, "concurrent isolate worker ready");

                            loop {
                                match rt.run_event_loop(Default::default()).await {
                                    Ok(()) => {
                                        tracing::warn!(worker = id, "event loop terminated");
                                        break;
                                    }
                                    Err(e) => {
                                        tracing::error!(worker = id, error = %e, "event loop error, continuing");
                                    }
                                }
                            }
                        });
                    })
                    .expect("failed to spawn isolate worker thread");

                Arc::new(WorkerHandle {
                    task_tx,
                    registry,
                    in_flight: Arc::new(AtomicUsize::new(0)),
                    bundle_key: Arc::new(std::sync::Mutex::new(None)),
                })
            })
            .collect();

        Self {
            worker_count: handles.len(),
            workers: handles,
            next_worker: Arc::new(AtomicUsize::new(0)),
            timeout_secs,
        }
    }

    /// Spawn a pool sized to 2× logical CPUs (min 2, max 16).
    pub fn default_sized() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = (cpus * 2).clamp(2, 16);
        tracing::info!(workers, "isolate pool started (concurrent workers)");
        Self::new(workers, 30)
    }

    pub fn workers(&self) -> usize { self.worker_count }

    /// Dispatch a function execution to the least-loaded available worker,
    /// preferring one that already has the same bundle loaded (affinity routing).
    ///
    /// `bundle_key` is typically the function's code hash or function_id. When
    /// provided, the pool first looks for a worker whose current bundle matches,
    /// then falls back to least-loaded. This maximises warm-isolate reuse: the
    /// same V8 heap already has the module evaluated.
    pub async fn execute(
        &self,
        code:           String,
        secrets:        HashMap<String, String>,
        payload:        serde_json::Value,
        execution_seed: i64,
        queue_ctx:      QueueContext,
        db_ctx:         DbContext,
        bundle_key:     Option<String>,
    ) -> Result<ExecutionResult, String> {
        // ── Pick worker via bundle-key affinity then least-loaded fallback ──
        let start = self.next_worker.fetch_add(1, Ordering::Relaxed);

        let worker: &Arc<WorkerHandle> = if let Some(ref key) = bundle_key {
            // 1. Try to find an idle worker already serving this bundle
            let affinity = (0..self.worker_count)
                .map(|i| &self.workers[(start + i) % self.worker_count])
                .find(|w| {
                    let bk = w.bundle_key.lock().unwrap_or_else(|p| p.into_inner());
                    bk.as_deref() == Some(key.as_str()) && w.in_flight.load(Ordering::Relaxed) == 0
                });

            if let Some(w) = affinity {
                w
            } else {
                // 2. Fall back to least-loaded (prefer affinity match over pure idle)
                let best_match = (0..self.worker_count)
                    .map(|i| &self.workers[(start + i) % self.worker_count])
                    .min_by_key(|w| {
                        let load = w.in_flight.load(Ordering::Relaxed);
                        let has_key = {
                            let bk = w.bundle_key.lock().unwrap_or_else(|p| p.into_inner());
                            bk.as_deref() == Some(key.as_str())
                        };
                        // Sort by: key mismatch first, then load
                        (if has_key { 0usize } else { 1usize }, load)
                    });
                best_match.unwrap()
            }
        } else {
            // No bundle key: pure least-loaded
            (0..self.worker_count)
                .map(|i| &self.workers[(start + i) % self.worker_count])
                .min_by_key(|w| w.in_flight.load(Ordering::Relaxed))
                .unwrap()
        };

        let request_id = Uuid::new_v4().to_string();
        let (reply_tx, reply_rx) = oneshot::channel::<Result<serde_json::Value, String>>();

        // Register reply channel before injecting the task
        {
            let mut reg = worker.registry.lock()
                .map_err(|_| "registry lock poisoned".to_string())?;
            reg.insert(request_id.clone(), reply_tx);
        }

        // Build task JSON (carries everything op_next_task delivers to JS)
        let task_json = serde_json::json!({
            "request_id":       request_id,
            "code":             code,
            "secrets":          secrets,
            "payload":          payload,
            "execution_seed":   execution_seed,
            "queue_url":        queue_ctx.queue_url,
            "api_url":          queue_ctx.api_url,
            "service_token":    queue_ctx.service_token,
            "project_id":       queue_ctx.project_id.map(|p| p.to_string()),
            "data_engine_url":  db_ctx.data_engine_url,
            "database":         db_ctx.database,
        });

        worker.task_tx.send(task_json).await
            .map_err(|_| "isolate worker task channel closed".to_string())?;

        // Record this worker's bundle key for future affinity routing
        if let Some(key) = bundle_key {
            if let Ok(mut bk) = worker.bundle_key.lock() {
                *bk = Some(key);
            }
        }

        worker.in_flight.fetch_add(1, Ordering::Relaxed);

        let result = timeout(
            Duration::from_secs(self.timeout_secs + 5),
            reply_rx,
        ).await;

        worker.in_flight.fetch_sub(1, Ordering::Relaxed);

        match result {
            Ok(Ok(val)) => val.map(|v| {
                let output = v.get("result").cloned().unwrap_or_else(|| v.clone());
                let logs = v.get("logs")
                    .and_then(|l| serde_json::from_value(l.clone()).ok())
                    .unwrap_or_default();
                ExecutionResult { output, logs }
            }),
            Ok(Err(_)) => Err("worker dropped reply channel".to_string()),
            Err(_) => {
                // Clean up the registry entry to avoid leaking the sender
                if let Ok(mut reg) = worker.registry.lock() {
                    reg.remove(&request_id);
                }
                Err(format!("function execution timed out after {} seconds", self.timeout_secs))
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_queue_ctx() -> QueueContext {
        QueueContext {
            queue_url:     "http://127.0.0.1:0".to_string(),
            api_url:       "http://127.0.0.1:0".to_string(),
            service_token: "test".to_string(),
            project_id:    None,
            client:        reqwest::Client::new(),
        }
    }

    fn test_db_ctx() -> DbContext {
        DbContext {
            data_engine_url: "http://127.0.0.1:0".to_string(),
            service_token:   "test".to_string(),
            database:        String::new(),
            client:          reqwest::Client::new(),
        }
    }

    // ── construction ──────────────────────────────────────────────────────

    #[test]
    fn new_pool_reports_worker_count() {
        let pool = IsolatePool::new(2, 30);
        assert_eq!(pool.workers(), 2);
    }

    #[test]
    fn minimum_one_worker_when_zero_given() {
        let pool = IsolatePool::new(0, 30);
        assert_eq!(pool.workers(), 1);
    }

    #[test]
    fn default_sized_has_at_least_two_workers() {
        let pool = IsolatePool::default_sized();
        assert!(pool.workers() >= 2);
    }

    // ── basic execution ───────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_simple_js_returns_value() {
        let pool = IsolatePool::new(1, 30);
        let code = r#"__fluxbase_fn = async (ctx) => "hello";"#;

        let res = pool.execute(
            code.to_string(),
            HashMap::new(),
            serde_json::Value::Null,
            0,
            test_queue_ctx(),
            test_db_ctx(),
            None,
        ).await;

        assert!(res.is_ok(), "expected Ok, got: {:?}", res.err());
        assert_eq!(res.unwrap().output, serde_json::json!("hello"));
    }

    #[tokio::test]
    async fn execute_passes_payload() {
        let pool = IsolatePool::new(1, 30);
        let code = r#"__fluxbase_fn = async (ctx) => ctx.payload.x * 2;"#;

        let res = pool.execute(
            code.to_string(),
            HashMap::new(),
            serde_json::json!({"x": 21}),
            0,
            test_queue_ctx(),
            test_db_ctx(),
            None,
        ).await.unwrap();

        assert_eq!(res.output, serde_json::json!(42));
    }

    #[tokio::test]
    async fn execute_captures_logs() {
        let pool = IsolatePool::new(1, 30);
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                ctx.log("pool log test", "warn");
                return { result: true };
            };
        "#;
        let res = pool.execute(
            code.to_string(),
            HashMap::new(),
            serde_json::Value::Null,
            0,
            test_queue_ctx(),
            test_db_ctx(),
            None,
        ).await.unwrap();

        assert!(!res.logs.is_empty());
        assert_eq!(res.logs[0].message, "pool log test");
        assert_eq!(res.logs[0].level,   "warn");
    }

    #[tokio::test]
    async fn execute_js_error_returns_err() {
        let pool = IsolatePool::new(1, 30);
        let code = r#"__fluxbase_fn = async (ctx) => { throw new Error("pool err"); };"#;

        let res = pool.execute(
            code.to_string(),
            HashMap::new(),
            serde_json::Value::Null,
            0,
            test_queue_ctx(),
            test_db_ctx(),
            None,
        ).await;

        assert!(res.is_err());
        assert!(res.unwrap_err().contains("pool err"));
    }

    // ── concurrency ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_multiple_concurrent_tasks() {
        let pool = IsolatePool::new(2, 30);
        let code = r#"__fluxbase_fn = async (ctx) => ctx.payload.n;"#;

        let mut handles = vec![];
        for n in 0u32..8 {
            let p = pool.clone();
            let c = code.to_string();
            handles.push(tokio::spawn(async move {
                p.execute(c, HashMap::new(), serde_json::json!({"n": n}), 0,
                    QueueContext {
                        queue_url: "http://127.0.0.1:0".to_string(),
                        api_url:   "http://127.0.0.1:0".to_string(),
                        service_token: "t".to_string(),
                        project_id: None,
                        client: reqwest::Client::new(),
                    },
                    DbContext {
                        data_engine_url: "http://127.0.0.1:0".to_string(),
                        service_token:   "t".to_string(),
                        database:        String::new(),
                        client:          reqwest::Client::new(),
                    },
                    None,
                ).await
            }));
        }
        let results: Vec<_> = futures::future::join_all(handles).await;
        for r in results {
            assert!(r.is_ok(), "task join failed");
            assert!(r.unwrap().is_ok(), "execution failed");
        }
    }

    #[tokio::test]
    async fn pool_is_clone_and_send() {
        let pool = IsolatePool::new(1, 30);
        let clone = pool.clone();
        // Both should be able to execute
        let code = r#"__fluxbase_fn = async (ctx) => 1;"#;
        let _r = clone.execute(code.to_string(), HashMap::new(),
            serde_json::Value::Null, 0, test_queue_ctx(), test_db_ctx(), None).await;
    }

    // ── deterministic replay ──────────────────────────────────────────────

    #[tokio::test]
    async fn same_seed_produces_same_output() {
        let pool = IsolatePool::new(1, 30);
        // Use ctx.uuid() — the per-task seeded method — not crypto.randomUUID()
        // which in concurrent mode is V8's native non-seeded implementation.
        let code = r#"__fluxbase_fn = async (ctx) => ctx.uuid();"#;

        let r1 = pool.execute(code.to_string(), HashMap::new(),
            serde_json::Value::Null, 42, test_queue_ctx(), test_db_ctx(), None).await.unwrap();
        let r2 = pool.execute(code.to_string(), HashMap::new(),
            serde_json::Value::Null, 42, test_queue_ctx(), test_db_ctx(), None).await.unwrap();

        assert_eq!(r1.output, r2.output,
            "same execution seed must produce same UUID for deterministic replay");
    }

    // ── bundle-key affinity ───────────────────────────────────────────────

    #[tokio::test]
    async fn affinity_key_is_recorded_on_worker() {
        let pool = IsolatePool::new(2, 30);
        let code = r#"__fluxbase_fn = async (ctx) => 1;"#;

        let _r = pool.execute(
            code.to_string(),
            HashMap::new(),
            serde_json::Value::Null,
            0,
            test_queue_ctx(),
            test_db_ctx(),
            Some("fn_abc123".to_string()),
        ).await;

        // At least one worker should have recorded the bundle key
        let has_key = pool.workers.iter().any(|w| {
            w.bundle_key.lock().unwrap().as_deref() == Some("fn_abc123")
        });
        assert!(has_key, "expected bundle key to be recorded on a worker");
    }

    #[tokio::test]
    async fn affinity_routes_same_key_to_same_worker() {
        // 3 workers; send 6 requests for the same bundle_key.
        // After the first request, subsequent ones should route to the same worker.
        let pool = IsolatePool::new(3, 30);
        let code = r#"__fluxbase_fn = async (ctx) => 42;"#;

        for _ in 0..6 {
            let r = pool.execute(
                code.to_string(),
                HashMap::new(),
                serde_json::Value::Null,
                0,
                test_queue_ctx(),
                test_db_ctx(),
                Some("fn_affinity_test".to_string()),
            ).await;
            assert!(r.is_ok());
        }

        // Exactly one worker should carry the bundle key
        let keyed: Vec<_> = pool.workers.iter()
            .filter(|w| w.bundle_key.lock().unwrap().as_deref() == Some("fn_affinity_test"))
            .collect();
        assert_eq!(keyed.len(), 1, "bundle key should be pinned to exactly one worker");
    }
}
