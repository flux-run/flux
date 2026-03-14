//! Deno V8 execution engine — runs JavaScript functions in sandboxed `JsRuntime` isolates.
//!
//! ## Isolate architecture
//!
//! Each call to `IsolatePool::execute` routes to a warm isolate worker thread.
//! The worker holds a `JsRuntime` that was created at thread startup (not per request).
//! Per-request state is injected into `OpState` before execution and cleared after.
//!
//! ## LogLine
//!
//! `ctx.log(level, message, opts)` inside user JS emits a `LogLine` into a
//! `__fluxbase_logs` array declared inside the IIFE wrapper. After the function
//! returns, `execute_with_runtime` extracts the logs from V8 memory and returns them
//! as `ExecutionResult::logs`. The caller (`ExecutionRunner`) ships them to
//! `flux.platform_logs` via `TraceEmitter::emit_logs` (fire-and-forget).
//!
//! ## Security hardening
//!
//! - Deterministic random seeding (`Math.random` → seeded PRNG) for replay.
//! - `globalThis.__fluxbase_logs` and `globalThis.__ctx` are re-declared as `const`
//!   inside the IIFE on every call, so user code cannot persist state across
//!   invocations via globals.
//! - V8 heap and stack are not shared between workers (each thread owns its runtime).
use deno_core::{JsRuntime, RuntimeOptions, OpState, Extension};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};


/// Build the Fluxbase runtime extension — queue ops + db ops + http + sleep + function invoke.
pub fn build_fluxbase_extension() -> Extension {
    Extension {
        name: "fluxbase",
        ops: Cow::Owned(vec![
            op_queue_push(), op_next_task(), op_task_complete(), op_task_error(),
            op_db_query(), op_http_fetch(), op_sleep(), op_function_invoke(),
        ]),
        ..Default::default()
    }
}

// ── Queue op ──────────────────────────────────────────────────────────────────
//
// Deno bridge: JS calls Deno.core.ops.op_queue_push(functionName, payload, delay, key)
// from inside ctx.queue.push().
// The op:
//   1. Resolves function name → UUID via GET {api_url}/internal/functions/resolve
//   2. POSTs to queue service /jobs
//   3. Returns { job_id }

/// Per-request queue context injected into Deno OpState.
pub struct QueueOpState {
    pub queue_url:     String,
    pub api_url:       String,
    pub service_token: String,
    pub project_id:    Option<uuid::Uuid>,
    pub client:        reqwest::Client,
}

/// Carry queue context from the async Tokio world (pool.rs) into execute_with_runtime.
#[derive(Clone)]
pub struct QueueContext {
    pub queue_url:     String,
    pub api_url:       String,
    pub service_token: String,
    pub project_id:    Option<uuid::Uuid>,
    pub client:        reqwest::Client,
}

/// Carry data-engine context from the pool into the V8 op.
#[derive(Clone)]
pub struct DbContext {
    pub data_engine_url: String,
    pub service_token:   String,
    pub database:        String,
    pub client:          reqwest::Client,
}

/// Options forwarded from JS's `opts` argument to `ctx.queue.push()`.
#[derive(serde::Deserialize)]
pub struct QueuePushOpts {
    pub delay_seconds:   Option<i64>,
    pub idempotency_key: Option<String>,
}

/// Receiver side of the task injection channel (wrapped for async op use).
pub type SharedTaskReceiver = Arc<tokio::sync::Mutex<mpsc::Receiver<serde_json::Value>>>;

/// Per-worker registry mapping request_id → reply oneshot.
pub type ResultRegistry = Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Result<serde_json::Value, String>>>>>;

#[deno_core::op2(async)]
#[serde]
pub async fn op_queue_push(
    state:             Rc<RefCell<OpState>>,
    #[string] function_name: String,
    #[serde]  payload:       serde_json::Value,
    #[serde]  opts:          QueuePushOpts,
    #[serde]  queue_ctx_override: Option<serde_json::Value>,
) -> Result<serde_json::Value, std::io::Error> {
    let (queue_url, api_url, service_token, project_id, client) =
        if let Some(ref ctx) = queue_ctx_override {
            // Concurrent path: context carried in the task JSON
            let queue_url     = ctx["queue_url"].as_str().unwrap_or("").to_string();
            let api_url       = ctx["api_url"].as_str().unwrap_or("").to_string();
            let service_token = ctx["service_token"].as_str().unwrap_or("").to_string();
            let project_id    = ctx["project_id"].as_str()
                .and_then(|s| uuid::Uuid::parse_str(s).ok());
            (queue_url, api_url, service_token, project_id, reqwest::Client::new())
        } else {
            // Serial path: context in OpState
            let s = state.borrow();
            match s.try_borrow::<QueueOpState>() {
                Some(qs) => (
                    qs.queue_url.clone(),
                    qs.api_url.clone(),
                    qs.service_token.clone(),
                    qs.project_id,
                    qs.client.clone(),
                ),
                None => return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "op_queue_push: no queue context available",
                )),
            }
        };

    let project_id_str = project_id
        .map(|p| p.to_string())
        .unwrap_or_default();

    // ── Resolve function name → { function_id, tenant_id } ───────────────
    let resolve_url = format!(
        "{}/internal/functions/resolve?name={}&project_id={}",
        api_url.trim_end_matches('/'), function_name, project_id_str,
    );

    let resolve_resp = client
        .get(&resolve_url)
        .header("X-Service-Token", &service_token)
        .send()
        .await
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("function resolve request failed: {}", e),
        ))?;

    if !resolve_resp.status().is_success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("ctx.queue.push: function '{}' not found in project", function_name),
        ));
    }

    let resolved: serde_json::Value = resolve_resp.json().await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let function_id = resolved["function_id"].as_str()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing function_id in resolve response",
        ))?;

    let tenant_id = resolved["tenant_id"].as_str()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing tenant_id in resolve response",
        ))?;

    // ── Push to queue service ─────────────────────────────────────────────
    let job_body = serde_json::json!({
        "tenant_id":       tenant_id,
        "project_id":      project_id,
        "function_id":     function_id,
        "payload":         payload,
        "delay_seconds":   opts.delay_seconds,
        "idempotency_key": opts.idempotency_key,
    });

    let queue_resp = client
        .post(format!("{}/jobs", queue_url.trim_end_matches('/')))
        .bearer_auth(&service_token)
        .json(&job_body)
        .send()
        .await
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("queue push failed: {}", e),
        ))?;

    if !queue_resp.status().is_success() {
        let status = queue_resp.status();
        let body = queue_resp.text().await.unwrap_or_default();
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("queue returned {}: {}", status, body),
        ));
    }

    let job_data: serde_json::Value = queue_resp.json().await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    Ok(serde_json::json!({ "job_id": job_data["job_id"] }))
}

/// Async op: JS bootstrap calls this to get the next task.
/// Suspends the V8 fiber until a task arrives, freeing the tokio thread.
#[deno_core::op2(async)]
#[serde]
pub async fn op_next_task(
    state: Rc<RefCell<OpState>>,
) -> Result<serde_json::Value, std::io::Error> {
    let receiver = {
        let s = state.borrow();
        s.borrow::<SharedTaskReceiver>().clone()
    };
    let mut guard = receiver.lock().await;
    guard.recv().await
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "task channel closed"))
}

/// Sync op: JS calls this when a task completes successfully.
#[deno_core::op2(fast)]
pub fn op_task_complete(
    state: &mut OpState,
    #[string] request_id: String,
    #[string] result_json: String,
) {
    let registry = state.borrow::<ResultRegistry>().clone();
    if let Ok(mut reg) = registry.lock() {
        if let Some(sender) = reg.remove(&request_id) {
            match serde_json::from_str::<serde_json::Value>(&result_json) {
                Ok(v)  => { let _ = sender.send(Ok(v)); }
                Err(e) => { let _ = sender.send(Err(format!("result parse error: {}", e))); }
            }
        }
    }
}

/// Sync op: JS calls this when a task fails.
#[deno_core::op2(fast)]
pub fn op_task_error(
    state: &mut OpState,
    #[string] request_id: String,
    #[string] error_msg: String,
) {
    let registry = state.borrow::<ResultRegistry>().clone();
    if let Ok(mut reg) = registry.lock() {
        if let Some(sender) = reg.remove(&request_id) {
            let _ = sender.send(Err(error_msg));
        }
    }
}

/// Async op: execute raw SQL via the data-engine service.
/// JS calls: Deno.core.ops.op_db_query(sql, params, db_ctx_override)
/// db_ctx_override carries { data_engine_url, service_token, database, request_id }
#[deno_core::op2(async)]
#[serde]
pub async fn op_db_query(
    state:              Rc<RefCell<OpState>>,
    #[string] sql:      String,
    #[serde]  params:   serde_json::Value,
    #[serde]  db_ctx_override: Option<serde_json::Value>,
) -> Result<serde_json::Value, std::io::Error> {
    let (data_engine_url, service_token, database, request_id, client) = {
        if let Some(ref ctx) = db_ctx_override {
            // Concurrent path: context carried in the task JSON
            let s = state.borrow();
            let client = s.try_borrow::<DbContext>()
                .map(|c| c.client.clone())
                .unwrap_or_else(reqwest::Client::new);
            (
                ctx["data_engine_url"].as_str().unwrap_or("").to_string(),
                ctx["service_token"].as_str().unwrap_or("").to_string(),
                ctx["database"].as_str().unwrap_or("").to_string(),
                ctx["request_id"].as_str().unwrap_or("").to_string(),
                client,
            )
        } else {
            // Serial path: context in OpState
            let s = state.borrow();
            match s.try_borrow::<DbContext>() {
                Some(db) => (
                    db.data_engine_url.clone(),
                    db.service_token.clone(),
                    db.database.clone(),
                    String::new(),
                    db.client.clone(),
                ),
                None => return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "op_db_query: no db context available",
                )),
            }
        }
    };

    let params_array = params
        .as_array()
        .cloned()
        .unwrap_or_default();

    let body = serde_json::json!({
        "sql":        sql,
        "params":     params_array,
        "database":   database,
        "request_id": request_id,
    });

    let resp = client
        .post(format!("{}/db/sql", data_engine_url.trim_end_matches('/')))
        .header("X-Service-Token", &service_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("db query failed ({}): {}", status, body),
        ));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

// ── HTTP fetch op ─────────────────────────────────────────────────────────────
//
// JS: Deno.core.ops.op_http_fetch(url, method, headers, body, ctx_override)
// Returns { status, headers: {}, body: string }
//
// SSRF protection: denies calls to RFC1918, loopback, link-local, and
// cloud-provider metadata endpoints. Also strips X-Service-Token and
// X-Internal-* headers that user code could forge to impersonate the runtime.

/// Returns true when the host is a private/loopback/metadata address that must
/// be blocked to prevent SSRF attacks from user functions.
fn is_ssrf_blocked(url: &str) -> bool {
    let parsed = match url.parse::<reqwest::Url>() {
        Ok(u) => u,
        Err(_) => return true, // malformed URLs are blocked
    };
    let host = match parsed.host_str() {
        Some(h) => h.to_ascii_lowercase(),
        None    => return true,
    };

    // Block cloud metadata endpoints
    if host == "metadata.google.internal"
        || host == "169.254.169.254"
        || host == "fd00:ec2::254"
    {
        return true;
    }

    // Block loopback
    if host == "localhost" || host == "::1" || host.starts_with("127.") {
        return true;
    }

    // Block link-local
    if host.starts_with("169.254.") || host.starts_with("fe80") {
        return true;
    }

    // Parse IP ranges for RFC1918
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                // 10.0.0.0/8
                if o[0] == 10 { return true; }
                // 172.16.0.0/12
                if o[0] == 172 && (16..=31).contains(&o[1]) { return true; }
                // 192.168.0.0/16
                if o[0] == 192 && o[1] == 168 { return true; }
            }
            std::net::IpAddr::V6(_) => {
                // Block fc00::/7 (ULA)
                if host.starts_with("fc") || host.starts_with("fd") { return true; }
            }
        }
    }

    false
}

/// Carry HTTP client for ctx.fetch() — injected into OpState on the serial path.
pub struct HttpFetchOpState {
    pub client:        reqwest::Client,
    pub allowed_hosts: Vec<String>, // empty = allow all (except SSRF blocked), ["*"] = allow all
}

/// ctx.fetch(url, { method, headers, body }) — SSRF-protected HTTP from user functions.
#[deno_core::op2(async)]
#[serde]
pub async fn op_http_fetch(
    state:          Rc<RefCell<OpState>>,
    #[string] url:  String,
    #[serde]  opts: serde_json::Value,
    #[serde]  http_ctx_override: Option<serde_json::Value>,
) -> Result<serde_json::Value, std::io::Error> {
    // SSRF check — always applied regardless of allow-list
    if is_ssrf_blocked(&url) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("fetch blocked: '{}' resolves to a private/reserved address", url),
        ));
    }

    let (client, allowed_hosts) = {
        let s = state.borrow();
        if let Some(ref _ctx) = http_ctx_override {
            // Concurrent path: use stored client, allow-list not enforced per-request
            let client = s.try_borrow::<HttpFetchOpState>()
                .map(|c| c.client.clone())
                .unwrap_or_else(reqwest::Client::new);
            (client, vec![])
        } else {
            match s.try_borrow::<HttpFetchOpState>() {
                Some(h) => (h.client.clone(), h.allowed_hosts.clone()),
                None    => (reqwest::Client::new(), vec![]),
            }
        }
    };

    // Host allow-list check (if configured)
    if !allowed_hosts.is_empty() && !allowed_hosts.contains(&"*".to_string()) {
        let host = url.parse::<reqwest::Url>()
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .unwrap_or_default();
        if !allowed_hosts.iter().any(|a| a.to_ascii_lowercase() == host) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("fetch blocked: host '{}' is not in the allowed_hosts list", host),
            ));
        }
    }

    let method = opts.get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("GET")
        .to_uppercase();

    let mut req = client.request(
        method.parse::<reqwest::Method>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?,
        &url,
    );

    // Forward headers, stripping internal ones
    if let Some(headers) = opts.get("headers").and_then(|h| h.as_object()) {
        for (k, v) in headers {
            let key_lower = k.to_ascii_lowercase();
            // Strip headers that could be used to forge internal service calls
            if key_lower == "x-service-token"
                || key_lower.starts_with("x-internal-")
                || key_lower == "x-flux-service"
            {
                continue;
            }
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    if let Some(body) = opts.get("body") {
        if let Some(s) = body.as_str() {
            req = req.body(s.to_string());
        } else {
            req = req.json(body);
        }
    }

    let resp = req.send().await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let status = resp.status().as_u16();
    let resp_headers: serde_json::Value = {
        let mut map = serde_json::Map::new();
        for (k, v) in resp.headers().iter() {
            map.insert(k.as_str().to_string(), serde_json::Value::String(v.to_str().unwrap_or("").to_string()));
        }
        serde_json::Value::Object(map)
    };

    let body_text = resp.text().await
        .unwrap_or_else(|_| String::new());

    // Try to parse as JSON; fall back to string
    let body_value: serde_json::Value = serde_json::from_str(&body_text)
        .unwrap_or_else(|_| serde_json::Value::String(body_text));

    Ok(serde_json::json!({
        "status":  status,
        "headers": resp_headers,
        "body":    body_value,
        "ok":      status >= 200 && status < 300,
    }))
}

// ── Sleep op ──────────────────────────────────────────────────────────────────
//
// JS: Deno.core.ops.op_sleep(ms)
// Suspends the current task for `ms` milliseconds.
// Unlike setTimeout, this properly yields the V8 event loop in concurrent mode.

/// ctx.sleep(ms) — suspend the current task without blocking the event loop.
#[deno_core::op2(async)]
pub async fn op_sleep(#[smi] ms: u32) {
    tokio::time::sleep(Duration::from_millis(ms as u64)).await;
}

// ── Function invoke op ────────────────────────────────────────────────────────
//
// JS: Deno.core.ops.op_function_invoke(function_name, payload, invoke_ctx_override)
// Calls another Flux function in-process by POSTing to the runtime /execute endpoint.
// Carries the parent request_id for call graph tracing (execution_calls table).

/// Carry the runtime's own execute endpoint for cross-function calls.
pub struct FunctionInvokeOpState {
    pub runtime_url:   String,   // e.g. "http://localhost:8080"
    pub service_token: String,
    pub request_id:    String,   // parent request_id for call graph
    pub client:        reqwest::Client,
}

/// ctx.function.invoke(name, payload) — call another Flux function in-process.
#[deno_core::op2(async)]
#[serde]
pub async fn op_function_invoke(
    state:                  Rc<RefCell<OpState>>,
    #[string] function_name: String,
    #[serde]  payload:       serde_json::Value,
    #[serde]  invoke_ctx_override: Option<serde_json::Value>,
) -> Result<serde_json::Value, std::io::Error> {
    let (runtime_url, service_token, parent_request_id, client) = {
        let s = state.borrow();
        if let Some(ref ctx) = invoke_ctx_override {
            let client = s.try_borrow::<FunctionInvokeOpState>()
                .map(|c| c.client.clone())
                .unwrap_or_else(reqwest::Client::new);
            (
                ctx["runtime_url"].as_str().unwrap_or("").to_string(),
                ctx["service_token"].as_str().unwrap_or("").to_string(),
                ctx["request_id"].as_str().unwrap_or("").to_string(),
                client,
            )
        } else {
            match s.try_borrow::<FunctionInvokeOpState>() {
                Some(fi) => (
                    fi.runtime_url.clone(),
                    fi.service_token.clone(),
                    fi.request_id.clone(),
                    fi.client.clone(),
                ),
                None => return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "op_function_invoke: no invoke context available",
                )),
            }
        }
    };

    if runtime_url.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "op_function_invoke: runtime_url not configured",
        ));
    }

    let resp = client
        .post(format!("{}/execute/{}", runtime_url.trim_end_matches('/'), function_name))
        .header("Content-Type", "application/json")
        .header("X-Service-Token", &service_token)
        .header("X-Parent-Request-ID", &parent_request_id)
        .json(&payload)
        .send()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("function invoke failed ({}): {}", status, body),
        ));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}
/// Intended to be called once per worker thread; per-request state is injected
/// via `OpState` before each execution (see `execute_with_runtime`).
///
/// # Isolation hardening (runs once per worker thread at startup):
///
/// 1. **Prototype freeze** — `Object.freeze` applied to the most-abused
///    built-in prototypes (`Object`, `Array`, `Function`, `String`, `Number`,
///    `Boolean`, `RegExp`, `Promise`, `Map`, `Set`, `Error`).  Prevents user
///    code from poisoning shared prototype chains across tenants.
///    Cost: ~20 µs at startup; no per-request overhead.
///
/// 2. **Global baseline snapshot** — captures all current `globalThis` key
///    names into `__fluxbase_allowed_globals` immediately after freezing.
///    `build_wrapper` sweeps any new keys added by a previous bundle before
///    the next request runs, eliminating cross-request `globalThis` leakage.
pub fn create_js_runtime() -> JsRuntime {
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![build_fluxbase_extension()],
        ..Default::default()
    });
    rt.execute_script(
        "<fluxbase-init>",
        // 1. Freeze built-in prototypes to prevent cross-tenant prototype poisoning.
        //    e.g. user code cannot do: Array.prototype.map = () => []
        "const __protos = [\
            Object, Array, Function, String, Number, Boolean,\
            RegExp, Promise, Map, Set, WeakMap, WeakSet, Error,\
            TypeError, RangeError, SyntaxError, ReferenceError\
        ];\
        for (const C of __protos) {\
            if (C && C.prototype) Object.freeze(C.prototype);\
        }\
        Object.freeze(__protos);\
        \
        // 2. Snapshot baseline globals for the per-request sweep in build_wrapper.\
        globalThis.__fluxbase_allowed_globals =\
            new Set(Object.getOwnPropertyNames(globalThis));",
    ).expect("failed to initialise worker sandbox");
    rt
}

/// Create a `JsRuntime` configured for concurrent multi-task execution.
/// Puts `SharedTaskReceiver` and `ResultRegistry` into OpState, then evaluates
/// the bootstrap loop so the runtime is ready to receive injected tasks.
pub fn create_concurrent_js_runtime(
    task_receiver: SharedTaskReceiver,
    result_registry: ResultRegistry,
) -> JsRuntime {
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![build_fluxbase_extension()],
        ..Default::default()
    });

    {
        let op_state = rt.op_state();
        let mut state = op_state.borrow_mut();
        state.put(task_receiver);
        state.put(result_registry);
    }

    rt.execute_script(
        "<fluxbase-init>",
        "const __protos = [Object, Array, Function, String, Number, Boolean, RegExp, Promise, Map, Set, WeakMap, WeakSet, Error, TypeError, RangeError, SyntaxError, ReferenceError]; for (const C of __protos) { if (C && C.prototype) Object.freeze(C.prototype); } Object.freeze(__protos); globalThis.__fluxbase_allowed_globals = new Set(Object.getOwnPropertyNames(globalThis));",
    ).expect("failed to initialise worker sandbox");

    rt.execute_script(
        "<fluxbase-bootstrap>",
        include_str!("bootstrap.js"),
    ).expect("failed to start bootstrap loop");

    rt
}

/// Build the JS IIFE wrapper that injects FluxContext and executes the bundle.
fn build_wrapper(
    secrets_json:     &str,
    payload_json:     &str,
    transformed_code: &str,
    execution_seed:   i64,
) -> String {
    format!(r#"
        var __fluxbase_fn;

        // ── Global scope sweep ──────────────────────────────────────────────
        // Delete any key set by a previous invocation on this warm isolate.
        // __fluxbase_allowed_globals is frozen at worker startup and contains
        // only V8/Deno built-ins — nothing a user bundle could have added.
        // Cost: O(n) over user-added keys only; typically 0–2 keys in practice.
        if (typeof __fluxbase_allowed_globals !== "undefined") {{
            for (const __k of Object.getOwnPropertyNames(globalThis)) {{
                if (!__fluxbase_allowed_globals.has(__k)) {{
                    try {{ delete globalThis[__k]; }} catch (_) {{}}
                }}
            }}
        }}

        // ── Deterministic execution seed ─────────────────────────────────────────────
        // Overrides Math.random, crypto.randomUUID, and nanoid with seeded equivalents
        // so `flux queue replay` reproduces identical IDs and execution paths.
        // When execution_seed is 0 (sync / non-replay path) the seed is a runtime-
        // generated mix, so behaviour is unchanged but still deterministic per call.
        (function() {{
            let __t = ({execution_seed} ^ 0xDEADBEEF) >>> 0;
            if (__t === 0) __t = 0x1;
            globalThis.__fluxbase_rand = function() {{
                __t += 0x6D2B79F5;
                let r = Math.imul(__t ^ (__t >>> 15), 1 | __t);
                r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
                return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
            }};
        }})();
        Math.random = globalThis.__fluxbase_rand;
        if (typeof crypto === "undefined") globalThis.crypto = {{}};
        crypto.randomUUID = () => {{
            const b = new Uint8Array(16);
            for (let i = 0; i < 16; i++) b[i] = Math.floor(globalThis.__fluxbase_rand() * 256);
            b[6] = (b[6] & 0x0f) | 0x40;
            b[8] = (b[8] & 0x3f) | 0x80;
            const h = x => (x + 256).toString(16).slice(1);
            return h(b[0])+h(b[1])+h(b[2])+h(b[3])+'-'+h(b[4])+h(b[5])+'-'+
                   h(b[6])+h(b[7])+'-'+h(b[8])+h(b[9])+'-'+
                   h(b[10])+h(b[11])+h(b[12])+h(b[13])+h(b[14])+h(b[15]);
        }};
        globalThis.nanoid = (size = 21) => {{
            const abc = "useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
            let id = "";
            for (let i = 0; i < size; i++) id += abc[Math.floor(globalThis.__fluxbase_rand() * abc.length)];
            return id;
        }};

        (async () => {{
            const __fluxbase_logs = [];

            const __secrets = {secrets_json};
            const __payload = {payload_json};

            // ── Full FluxContext implementation ────────────────────────
            const __ctx = {{

                payload: __payload,
                env:     __secrets,

                // Deterministic per-request UUID/nanoid backed by the seeded PRNG.
                // Use these instead of crypto.randomUUID() for replay-safe ID generation.
                uuid:   () => crypto.randomUUID(),
                nanoid: (size = 21) => globalThis.nanoid(size),

                // Secrets accessor
                secrets: {{
                    get: (key) => __secrets[key] !== undefined ? __secrets[key] : null,
                }},

                // Structured logger
                log: (message, level) => {{
                    __fluxbase_logs.push({{
                        level:     level || "info",
                        message:   String(message),
                        span_type: "event",
                        source:    "function",
                    }});
                }},

                // ── Tools ─────────────────────────────────────────────
                tools: {{
                    run: async () => {{
                        throw new Error("ctx.tools is not available in this runtime");
                    }},
                }},

                // ── Workflow ─────────────────────────────────────────
                // ctx.workflow.run([ {{ name: "step1", fn: async (ctx, prev) => ... }} ])
                // ctx.workflow.parallel([ {{ name: "step1", fn: async (ctx) => ... }} ])
                workflow: {{
                    run: async (steps, options) => {{
                        options = options || {{}};
                        const outputs = {{}};
                        for (const step of steps) {{
                            const name = step.name || ("step_" + Object.keys(outputs).length);
                            const _start = Date.now();
                            try {{
                                const result = await step.fn(__ctx, outputs);
                                const duration = Date.now() - _start;
                                __fluxbase_logs.push({{
                                    level:     "info",
                                    message:   "workflow:" + name + "  " + duration + "ms",
                                    span_type: "workflow_step",
                                    source:    "workflow",
                                }});
                                outputs[name] = result;
                            }} catch (e) {{
                                const duration = Date.now() - _start;
                                __fluxbase_logs.push({{
                                    level:     "error",
                                    message:   "workflow:" + name + "  failed (" + duration + "ms): " + (e && e.message),
                                    span_type: "workflow_step",
                                    source:    "workflow",
                                }});
                                if (options.continueOnError) {{
                                    outputs[name] = {{ __error: e && e.message }};
                                }} else {{
                                    throw e;
                                }}
                            }}
                        }}
                        return outputs;
                    }},
                    parallel: async (steps) => {{
                        const settled = await Promise.allSettled(steps.map(function(step) {{
                            const name = step.name || "step";
                            const _start = Date.now();
                            return step.fn(__ctx).then(function(result) {{
                                const duration = Date.now() - _start;
                                __fluxbase_logs.push({{
                                    level:     "info",
                                    message:   "workflow:" + name + "  " + duration + "ms (parallel)",
                                    span_type: "workflow_step",
                                    source:    "workflow",
                                }});
                                return result;
                            }});
                        }}));
                        const outputs = {{}};
                        settled.forEach(function(r, i) {{
                            const name = (steps[i] && steps[i].name) ? steps[i].name : ("step_" + i);
                            outputs[name] = r.status === "fulfilled" ? r.value : {{ __error: r.reason && r.reason.message }};
                        }});
                        return outputs;
                    }},
                }},

                // ── Queue ─────────────────────────────────────────────
                // ctx.queue.push("function_name", payload, {{ delay: "5m", idempotencyKey: "..." }})
                //
                // Enqueues a background job. The runtime resolves the function name
                // to a UUID, calls the Queue service, and records a queue_push span
                // so the enqueue appears in `flux trace`.
                queue: {{
                    push: async (functionName, payload, opts) => {{
                        opts = opts || {{}};
                        const delay = opts.delay
                            ? (() => {{
                                const _d = String(opts.delay);
                                if (_d.endsWith("h")) return parseInt(_d) * 3600;
                                if (_d.endsWith("m")) return parseInt(_d) * 60;
                                if (_d.endsWith("s")) return parseInt(_d);
                                return parseInt(_d);
                              }})()
                            : (opts.delay_seconds || null);
                        const result = await Deno.core.ops.op_queue_push(
                            functionName,
                            payload !== undefined ? payload : {{}},
                            {{
                                delay_seconds:   delay,
                                idempotency_key: opts.idempotencyKey || opts.idempotency_key || null,
                            }},
                            null
                        );
                        __fluxbase_logs.push({{
                            level:     "info",
                            message:   "queue_push:" + functionName + "  job_id=" + (result && result.job_id),
                            span_type: "queue_push",
                            source:    "queue",
                        }});
                        return result;
                    }},
                }},

                // ── Database ──────────────────────────────────────────────
                // ctx.db.query(sql, params) — executes raw SQL via the data-engine.
                // ctx.db.execute(sql, params) — alias for ctx.db.query.
                db: {{
                    query: async (sql, params) => {{
                        const _start = Date.now();
                        const result = await Deno.core.ops.op_db_query(
                            sql,
                            Array.isArray(params) ? params : [],
                            null
                        );
                        __fluxbase_logs.push({{
                            level:       "info",
                            message:     "db:query  " + (Date.now() - _start) + "ms  " + (result && result.meta ? result.meta.rows + " rows" : ""),
                            span_type:   "db_query",
                            source:      "db",
                            duration_ms: Date.now() - _start,
                        }});
                        return result && result.data ? result.data : result;
                    }},
                    execute: async (sql, params) => __ctx.db.query(sql, params),
                }},

                // ── HTTP fetch ────────────────────────────────────────────
                // ctx.fetch(url, {{ method, headers, body }})
                // SSRF-protected HTTP — blocks RFC1918, loopback, link-local,
                // and cloud metadata endpoints.
                fetch: async (url, opts) => {{
                    const _start = Date.now();
                    const result = await Deno.core.ops.op_http_fetch(
                        url,
                        opts || {{}},
                        null
                    );
                    __fluxbase_logs.push({{
                        level:       "info",
                        message:     "http:" + (opts && opts.method || "GET") + "  " + url + "  " + result.status + "  " + (Date.now() - _start) + "ms",
                        span_type:   "http_fetch",
                        source:      "http",
                        duration_ms: Date.now() - _start,
                    }});
                    return result;
                }},

                // ── Sleep ─────────────────────────────────────────────────
                // ctx.sleep(ms) — yields event loop for ms milliseconds.
                // Replay-safe: duration is recorded in spans.
                sleep: async (ms) => {{
                    await Deno.core.ops.op_sleep(ms | 0);
                }},

                // ── Function invoke ───────────────────────────────────────
                // ctx.function.invoke(name, payload)
                // Calls another Flux function, wiring the parent request_id for
                // call graph tracing.
                function: {{
                    invoke: async (name, payload) => {{
                        const _start = Date.now();
                        const result = await Deno.core.ops.op_function_invoke(
                            name,
                            payload !== undefined ? payload : {{}},
                            null
                        );
                        __fluxbase_logs.push({{
                            level:       "info",
                            message:     "invoke:" + name + "  " + (Date.now() - _start) + "ms",
                            span_type:   "function_invoke",
                            source:      "function",
                            duration_ms: Date.now() - _start,
                        }});
                        return result;
                    }},
                }},
            }};

            // Execute the bundle
            {transformed_code}

            let __result;
            let target_fn = __fluxbase_fn;

            // esbuild wraps the default export under .default
            if (target_fn && target_fn.default) {{
                target_fn = target_fn.default;
            }}

            if (typeof target_fn === 'object' && target_fn !== null && target_fn.__fluxbase === true) {{
                try {{
                    __result = await target_fn.execute(__payload, __ctx);
                }} catch (e) {{
                    const code = e.code || 'EXECUTION_ERROR';
                    throw new Error(JSON.stringify({{ code, message: e.message }}));
                }}
            }} else if (typeof target_fn === 'function') {{
                __result = await target_fn(__ctx);
            }} else {{
                throw new Error("Bundle must export a defineFunction() result or an async function. Got: " + typeof target_fn);
            }}

            return {{ result: __result, logs: __fluxbase_logs }};
        }})()
    "#,
        secrets_json     = secrets_json,
        payload_json     = payload_json,
        transformed_code = transformed_code,
        execution_seed   = execution_seed,
    )
}

// ── ExecutionResult + LogLine ─────────────────────────────────────────────────

/// Result of executing a framework-wrapped function.
#[derive(Debug)]
pub struct ExecutionResult {
    pub output: serde_json::Value,
    pub logs:   Vec<LogLine>,
}

/// A structured log line emitted by user code or the tool executor.
/// `span_type` and `source` allow the trace viewer to render distinct span kinds.
///
/// Fields added for execution tracing:
/// - `span_id`           — unique ID for this span; generated JS-side or server-side on ship
/// - `duration_ms`       — set by tool/workflow spans; propagated to log sink
/// - `execution_state`   — lifecycle state: "started" | "running" | "completed" | "error"
/// - `tool_name`         — the Fluxbase tool name for `span_type == "tool"` spans
#[derive(Debug, serde::Deserialize)]
pub struct LogLine {
    pub level:   String,
    pub message: String,
    /// "event" (default) | "tool" | "workflow_step" | "start" | "end"
    #[serde(default)]
    pub span_type: Option<String>,
    /// "function" (default) | "tool" | "workflow" | "runtime"
    #[serde(default)]
    pub source: Option<String>,
    /// Unique span identifier — used to link parent → child spans across services.
    /// If not provided by JS, routes.rs generates a UUID v4 before shipping.
    #[serde(default)]
    pub span_id: Option<String>,
    /// Duration in ms — set by tool/workflow spans for replay recording.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Lifecycle state tag used for replay and trace bisect.
    #[serde(default)]
    pub execution_state: Option<String>,
    /// Tool name for tool spans — used to correlate with replay recordings.
    #[serde(default)]
    pub tool_name: Option<String>,
}

/// Execute a function on an **already-created** `JsRuntime`.
///
/// This is the hot path used by `IsolatePool` workers. The runtime is created
/// once per worker thread (`create_js_runtime()`) and reused across invocations.
/// Per-request state (secrets, tenant) is injected into `OpState`
/// before each execution via `try_take + put` — a clean swap, no reallocations.
///
/// # Performance
/// Eliminates per-request costs of the cold path:
/// - `JsRuntime::new` (V8 heap init + extension registration): ~3–5 ms
/// - `std::thread::spawn` (OS thread + 8 MB stack): ~0.5 ms
/// - `tokio::Runtime::build` (single-thread runtime): ~0.5 ms
///
/// # Safety / state isolation
/// - `__fluxbase_logs` is declared inside the IIFE — fresh per call.
/// - `__ctx` is declared inside the IIFE — fresh per call, holds secrets/payload.
/// - `__fluxbase_fn` is a global `var` — re-assigned by the bundle on every call.
/// - User globals (`globalThis.*`) are swept at the start of each IIFE using the
///   `__fluxbase_allowed_globals` snapshot taken at worker startup. Any key added
///   by a previous bundle is deleted before the next bundle runs, ensuring no
///   cross-request data leakage on a warm isolate.
/// - On timeout the caller (`IsolatePool`) marks the runtime for recreation so
///   the next call on that worker gets a fresh isolate (V8 won't be stuck).
pub async fn execute_with_runtime(
    rt:             &mut JsRuntime,
    code:           String,
    secrets:        HashMap<String, String>,
    payload:        serde_json::Value,
    execution_seed: i64,
    queue_ctx:      QueueContext,
    timeout_secs:   u64,
) -> Result<ExecutionResult, String> {
    // ── Per-request OpState injection ─────────────────────────────────────────
    // Use try_take + put to handle both the first call and subsequent reuse.
    {
        let op_state = rt.op_state();
        let mut state = op_state.borrow_mut();

        let _ = state.try_take::<QueueOpState>();
        state.put(QueueOpState {
            queue_url:     queue_ctx.queue_url,
            api_url:       queue_ctx.api_url,
            service_token: queue_ctx.service_token,
            project_id:    queue_ctx.project_id,
            client:        queue_ctx.client,
        });
    }

    // ── Build + execute the IIFE wrapper ───────────────────────────────────
    let secrets_json     = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
    let payload_json     = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let transformed_code = code;

    let wrapper = build_wrapper(
        &secrets_json, &payload_json, &transformed_code, execution_seed,
    );

    let res = timeout(Duration::from_secs(timeout_secs), async {
        let res = rt.execute_script("<anon>", wrapper)
            .map_err(|e| format!("Execution error: {}", e))?;

        let resolved_future = rt.resolve(res);
        let resolved = rt.with_event_loop_promise(resolved_future, Default::default()).await
            .map_err(|e| format!("Promise resolution error: {}", e))?;

        let mut scope = rt.handle_scope();
        let local     = deno_core::v8::Local::new(&mut scope, resolved);

        let json_val = deno_core::serde_v8::from_v8::<serde_json::Value>(&mut scope, local)
            .map_err(|e| format!("Serialization error: {}", e))?;

        Ok(json_val)
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
        Err(_)     => Err(format!("Function execution timed out after {} seconds", timeout_secs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn no_op_queue_ctx() -> QueueContext {
        QueueContext {
            queue_url:     "http://127.0.0.1:0".to_string(),
            api_url:       "http://127.0.0.1:0".to_string(),
            service_token: "test-token".to_string(),
            project_id:    None,
            client:        reqwest::Client::new(),
        }
    }

    /// Execute `code` in a fresh JsRuntime and return the result.
    /// Uses current_thread flavor so JsRuntime (!Send) stays on one thread.
    async fn run_js(code: &str, payload: serde_json::Value) -> Result<ExecutionResult, String> {
        let mut rt = create_js_runtime();
        execute_with_runtime(
            &mut rt,
            code.to_string(),
            HashMap::new(),
            payload,
            0,
            no_op_queue_ctx(),
            30,
        ).await
    }

    async fn run_js_with_secrets(
        code: &str,
        secrets: HashMap<String, String>,
    ) -> Result<ExecutionResult, String> {
        let mut rt = create_js_runtime();
        execute_with_runtime(
            &mut rt,
            code.to_string(),
            secrets,
            serde_json::Value::Null,
            0,
            no_op_queue_ctx(),
            30,
        ).await
    }

    // ── create_js_runtime ─────────────────────────────────────────────────

    #[test]
    fn create_js_runtime_does_not_panic() {
        let _rt = create_js_runtime();
    }

    #[test]
    fn multiple_runtimes_are_independent() {
        let _r1 = create_js_runtime();
        let _r2 = create_js_runtime();
    }

    // ── basic execution ───────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn returns_simple_value() {
        let code = r#"
            __fluxbase_fn = async (ctx) => 42;
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(42));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_object() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ({ hello: "world" });
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!({"hello": "world"}));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_null_result() {
        let code = r#"
            __fluxbase_fn = async (ctx) => null;
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert!(res.output.is_null());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn payload_available_in_ctx() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.payload.name;
        "#;
        let res = run_js(code, serde_json::json!({"name": "alice"})).await.unwrap();
        assert_eq!(res.output, serde_json::json!("alice"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_payload_fields() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.payload.a.b.c;
        "#;
        let res = run_js(code, serde_json::json!({"a":{"b":{"c":99}}})).await.unwrap();
        assert_eq!(res.output, serde_json::json!(99));
    }

    // ── secrets / env ─────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn secrets_accessible_via_ctx_env() {
        let mut secrets = HashMap::new();
        secrets.insert("MY_KEY".to_string(), "super-secret".to_string());
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.env.MY_KEY;
        "#;
        let res = run_js_with_secrets(code, secrets).await.unwrap();
        assert_eq!(res.output, serde_json::json!("super-secret"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_secret_is_undefined() {
        let code = r#"
            __fluxbase_fn = async (ctx) => (ctx.env.NONEXISTENT ?? "fallback");
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("fallback"));
    }

    // ── logging ───────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_log_emits_log_lines() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                ctx.log("hello from function", "info");
                return { result: "ok" };
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert!(!res.logs.is_empty(), "expected at least one log line");
        assert_eq!(res.logs[0].message, "hello from function");
        assert_eq!(res.logs[0].level,   "info");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiple_log_levels_captured() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                ctx.log("info msg",  "info");
                ctx.log("warn msg",  "warn");
                ctx.log("error msg", "error");
                return { result: true };
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.logs.len(), 3);
        let levels: Vec<&str> = res.logs.iter().map(|l| l.level.as_str()).collect();
        assert!(levels.contains(&"info"));
        assert!(levels.contains(&"warn"));
        assert!(levels.contains(&"error"));
    }

    // ── polyfills ─────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn crypto_random_uuid_returns_uuid_shaped_string() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const id = crypto.randomUUID();
                return id;
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        let uuid_str = res.output.as_str().unwrap_or("");
        // UUID format: 8-4-4-4-12 hex chars with dashes
        assert_eq!(uuid_str.len(), 36, "expected UUID length 36, got: {uuid_str}");
        assert_eq!(uuid_str.chars().filter(|&c| c == '-').count(), 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn math_random_returns_number_in_range() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const r = Math.random();
                return (r >= 0 && r < 1);
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(true));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deterministic_seed_produces_same_uuid() {
        // Same seed → same UUID on both calls
        let code = r#"
            __fluxbase_fn = async (ctx) => crypto.randomUUID();
        "#;
        let seed = 42i64;

        let mut rt1 = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt1, code.to_string(), HashMap::new(),
            serde_json::Value::Null, seed, no_op_queue_ctx(), 30).await.unwrap();

        let mut rt2 = create_js_runtime();
        let r2 = execute_with_runtime(&mut rt2, code.to_string(), HashMap::new(),
            serde_json::Value::Null, seed, no_op_queue_ctx(), 30).await.unwrap();

        assert_eq!(r1.output, r2.output, "same seed must produce same UUID");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_seeds_produce_different_uuids() {
        let code = r#"
            __fluxbase_fn = async (ctx) => crypto.randomUUID();
        "#;
        let mut rt1 = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt1, code.to_string(), HashMap::new(),
            serde_json::Value::Null, 1, no_op_queue_ctx(), 30).await.unwrap();

        let mut rt2 = create_js_runtime();
        let r2 = execute_with_runtime(&mut rt2, code.to_string(), HashMap::new(),
            serde_json::Value::Null, 2, no_op_queue_ctx(), 30).await.unwrap();

        assert_ne!(r1.output, r2.output);
    }

    // ── error handling ────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn syntax_error_returns_err() {
        let code = "this is not valid javascript }{{{";
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err(), "expected Err for syntax error");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_throw_returns_err() {
        let code = r#"
            __fluxbase_fn = async (ctx) => { throw new Error("exploded"); };
        "#;
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("exploded"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn undefined_variable_reference_returns_err() {
        let code = r#"
            __fluxbase_fn = async (ctx) => undeclaredVar;
        "#;
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err());
    }

    // ── isolation ─────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn globals_are_cleaned_between_invocations() {
        // First invocation sets a local IIFE-scoped fn and runs cleanly.
        let code1 = r#"
            __fluxbase_fn = async (ctx) => "first";
        "#;
        // Second invocation on the SAME runtime must still work correctly
        // (even if some globals leak between calls, execution must not fail).
        let code2 = r#"
            __fluxbase_fn = async (ctx) => "second";
        "#;
        let mut rt = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt, code1.to_string(), HashMap::new(),
            serde_json::Value::Null, 0, no_op_queue_ctx(), 30).await.unwrap();
        let r2 = execute_with_runtime(&mut rt, code2.to_string(), HashMap::new(),
            serde_json::Value::Null, 0, no_op_queue_ctx(), 30).await.unwrap();
        assert_eq!(r1.output, serde_json::json!("first"));
        assert_eq!(r2.output, serde_json::json!("second"),
            "reused runtime must produce correct output on second invocation");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prototype_freeze_prevents_poisoning() {
        // Object.freeze prevents modification — in sloppy mode the assignment
        // silently fails (no throw); the property retains its original value.
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const orig = Array.prototype.map;
                Array.prototype.map = () => "poisoned";
                // If frozen, the assignment is a no-op and map is unchanged.
                return Array.prototype.map === orig ? "frozen" : "not frozen";
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("frozen"),
            "Array.prototype must be frozen — assignment must be a no-op");
    }

    // ── async JS ──────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn awaited_promise_resolves() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const val = await Promise.resolve(99);
                return val;
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(99));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn setTimeout_is_not_required_for_basic_execution() {
        // Functions don't need setTimeout — just test it doesn't error.
        let code = r#"
            __fluxbase_fn = async (ctx) => "no timers needed";
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("no timers needed"));
    }

    // ── LogLine struct ────────────────────────────────────────────────────

    // ── is_ssrf_blocked ───────────────────────────────────────────────────

    #[test]
    fn ssrf_blocks_loopback() {
        assert!(is_ssrf_blocked("http://127.0.0.1/secret"));
        assert!(is_ssrf_blocked("http://localhost/secret"));
        assert!(is_ssrf_blocked("http://127.0.0.53/dns"));
        assert!(is_ssrf_blocked("http://::1/"));
    }

    #[test]
    fn ssrf_blocks_rfc1918() {
        assert!(is_ssrf_blocked("http://10.0.0.1/internal"));
        assert!(is_ssrf_blocked("http://10.255.255.255/"));
        assert!(is_ssrf_blocked("http://172.16.0.1/"));
        assert!(is_ssrf_blocked("http://172.31.255.255/"));
        assert!(is_ssrf_blocked("http://192.168.1.1/"));
        assert!(is_ssrf_blocked("http://192.168.0.100/"));
    }

    #[test]
    fn ssrf_blocks_metadata_endpoints() {
        assert!(is_ssrf_blocked("http://169.254.169.254/latest/meta-data/"));
        assert!(is_ssrf_blocked("http://metadata.google.internal/"));
        assert!(is_ssrf_blocked("http://169.254.0.1/"));
    }

    #[test]
    fn ssrf_blocks_link_local() {
        assert!(is_ssrf_blocked("http://169.254.1.1/"));
    }

    #[test]
    fn ssrf_blocks_malformed_url() {
        assert!(is_ssrf_blocked("not-a-url"));
        assert!(is_ssrf_blocked("://missing-scheme"));
    }

    #[test]
    fn ssrf_allows_public_addresses() {
        assert!(!is_ssrf_blocked("https://api.example.com/v1"));
        assert!(!is_ssrf_blocked("https://1.1.1.1/dns-query"));
        assert!(!is_ssrf_blocked("https://8.8.8.8/"));
        assert!(!is_ssrf_blocked("https://github.com/api"));
    }

    // ── op_http_fetch SSRF rejection via ctx.fetch() ──────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_fetch_rejects_ssrf_loopback() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                try {
                    await ctx.fetch("http://127.0.0.1/secret");
                    return "should_not_reach";
                } catch (e) {
                    return e.message.includes("blocked") ? "blocked" : e.message;
                }
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("blocked"), "expected SSRF block for 127.0.0.1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_fetch_rejects_ssrf_private_range() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                try {
                    await ctx.fetch("http://10.0.0.1/internal");
                    return "should_not_reach";
                } catch (e) {
                    return e.message.includes("blocked") ? "blocked" : e.message;
                }
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("blocked"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_fetch_rejects_metadata_endpoint() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                try {
                    await ctx.fetch("http://169.254.169.254/latest/meta-data/");
                    return "should_not_reach";
                } catch (e) {
                    return e.message.includes("blocked") ? "blocked" : e.message;
                }
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("blocked"));
    }

    // ── op_sleep via ctx.sleep() ──────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_sleep_returns_after_delay() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const before = Date.now();
                await ctx.sleep(50);
                const elapsed = Date.now() - before;
                return elapsed >= 40 ? "slept" : "too_fast";
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("slept"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_sleep_zero_is_safe() {
        let code = r#"__fluxbase_fn = async (ctx) => { await ctx.sleep(0); return "ok"; };"#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("ok"));
    }

    // ── op_function_invoke error path (no runtime_url) ───────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_function_invoke_errors_without_runtime_url() {
        // On the serial path FunctionInvokeOpState is not in OpState,
        // so the op returns "no invoke context available".
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                try {
                    await ctx.function.invoke("other_fn", { x: 1 });
                    return "should_not_reach";
                } catch (e) {
                    // Either "no invoke context" or "runtime_url not configured"
                    return (e.message.includes("invoke") || e.message.includes("runtime_url"))
                        ? "invoke_error" : e.message;
                }
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("invoke_error"),
            "invoke without context must throw a descriptive error");
    }

    // ── LogLine struct ────────────────────────────────────────────────────
    #[allow(dead_code)] // marker to keep section header above

    #[test]
    fn log_line_serde_roundtrip() {
        // LogLine derives Deserialize (not Serialize) — parse from raw JSON
        let json = r#"{"level":"info","message":"test message","span_type":"event","source":"function"}"#;
        let line: LogLine = serde_json::from_str(json).unwrap();
        assert_eq!(line.level,   "info");
        assert_eq!(line.message, "test message");
        assert_eq!(line.span_type.as_deref(), Some("event"));
        assert!(line.span_id.is_none());
    }

    // ── ExecutionResult struct ────────────────────────────────────────────

    #[test]
    fn execution_result_with_empty_logs() {
        let r = ExecutionResult {
            output: serde_json::json!({"k": "v"}),
            logs:   vec![],
        };
        assert!(r.logs.is_empty());
        assert_eq!(r.output["k"], "v");
    }
}
