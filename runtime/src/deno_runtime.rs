use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::rc::Rc;

use anyhow::{Context, Result};
use deno_core::error::AnyError;
use deno_core::{JsRuntime, OpState, RuntimeOptions, op2};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::isolate_pool::ExecutionContext;

/// Maximum response body size: 10 MB.
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Blocked metadata hostnames (cloud provider instance metadata endpoints).
const BLOCKED_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "169.254.170.2",
];

/// Validate that a URL is safe to fetch — blocks SSRF to cloud metadata and private IPs.
fn validate_fetch_url(raw_url: &str) -> std::result::Result<(), AnyError> {
    let parsed = url::Url::parse(raw_url)
        .map_err(|e| deno_core::error::custom_error("TypeError", format!("invalid URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| deno_core::error::custom_error("TypeError", "invalid URL: no host"))?;

    for blocked in BLOCKED_HOSTS {
        if host == *blocked {
            return Err(deno_core::error::custom_error(
                "PermissionDenied",
                format!("fetch blocked: {host} is a restricted endpoint"),
            ));
        }
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip.is_loopback() || is_private_ip(&ip) {
            return Err(deno_core::error::custom_error(
                "PermissionDenied",
                "fetch blocked: private/loopback IP addresses are not allowed",
            ));
        }
    }

    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()          // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            || v4.is_link_local()    // 169.254.0.0/16
            || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()     // ::1
            || (v6.segments()[0] & 0xfe00) == 0xfc00  // fc00::/7 unique-local
            || (v6.segments()[0] & 0xffc0) == 0xfe80  // fe80::/10 link-local
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionMode {
    Live,
    Replay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchCheckpoint {
    pub call_index: u32,
    pub boundary: String,
    pub url: String,
    pub method: String,
    pub request: serde_json::Value,
    pub response: serde_json::Value,
    pub duration_ms: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

/// A virtual HTTP request fed into a server-mode isolate by the Rust host.
#[derive(Debug, Clone)]
pub struct NetRequest {
    pub req_id: String,
    pub method: String,
    pub url: String,
    /// JSON-encoded `[[name, value], ...]` header pairs.
    pub headers_json: String,
    pub body: String,
}

/// The response produced by the JS handler and captured via `op_net_respond`.
#[derive(Debug, Clone)]
pub struct NetResponse {
    pub status: u16,
    /// `(name, value)` header pairs.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct JsExecutionOutput {
    pub output: serde_json::Value,
    pub checkpoints: Vec<FetchCheckpoint>,
    pub error: Option<String>,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone)]
struct RuntimeExecutionState {
    context: ExecutionContext,
    call_index: u32,
    checkpoints: Vec<FetchCheckpoint>,
    /// Pre-recorded checkpoints for Replay mode, keyed by call_index.
    recorded: HashMap<u32, FetchCheckpoint>,
    /// First `Date.now()` seen in Live mode; returned verbatim in Replay mode.
    recorded_now_ms: Option<u64>,
    /// Console output captured during this execution.
    logs: Vec<LogEntry>,
    /// Random f64 values produced in Live mode; replayed in order in Replay mode.
    recorded_random: Vec<f64>,
    /// How many recorded_random values have been consumed so far in Replay mode.
    random_index: usize,
    /// UUID strings produced in Live mode; replayed in order in Replay mode.
    recorded_uuids: Vec<String>,
    /// How many recorded_uuids have been consumed so far in Replay mode.
    uuid_index: usize,
    /// Set to true when the user module calls `Deno.serve()`.
    is_server_mode: bool,
    /// Pending responses keyed by req_id, filled by `op_net_respond`.
    pending_responses: HashMap<String, NetResponse>,
}

deno_core::extension!(flux_runtime_ext, ops = [op_fetch, op_now, op_console, op_timer_delay, op_random, op_random_uuid, op_net_listen, op_net_respond]);

#[op2(async)]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] method: String,
    #[serde] body: Option<serde_json::Value>,
    #[serde] headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let original_url = url;

    let (request_id, call_index, mode, recorded_checkpoint, client) = {
        let mut state_ref = state.borrow_mut();
        let (request_id, index, mode, recorded) = {
            let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
            let idx = execution.call_index;
            execution.call_index = execution.call_index.saturating_add(1);
            let rec = execution.recorded.remove(&idx);
            (
                execution.context.request_id.clone(),
                idx,
                execution.context.mode.clone(),
                rec,
            )
        };
        let http_client = state_ref.borrow::<Client>().clone();
        (request_id, index, mode, recorded, http_client)
    };

    // In Replay mode, return the recorded response instead of making a live call.
    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let mut state_ref = state.borrow_mut();
                let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
                execution.checkpoints.push(FetchCheckpoint {
                    call_index,
                    boundary: checkpoint.boundary,
                    url: checkpoint.url,
                    method: checkpoint.method,
                    request: checkpoint.request,
                    response: response.clone(),
                    duration_ms: checkpoint.duration_ms,
                });
            }
            tracing::debug!(%request_id, %call_index, "replay: returned recorded response");
            return Ok(response);
        }
        // No recorded checkpoint for this index — fall through to live call.
        tracing::warn!(%request_id, %call_index, "replay: no recorded checkpoint, making live call");
    }

    // SSRF protection: block private/metadata IPs before making the request.
    validate_fetch_url(&original_url)?;

    let resolved_url = original_url.clone();
    let request_json = serde_json::json!({
        "url": original_url.clone(),
        "resolved_url": resolved_url.clone(),
        "method": method.clone(),
        "body": body.clone(),
        "headers": headers.clone(),
    });

    let started = std::time::Instant::now();
    let target_url = resolved_url;

    let response = make_http_request(&client, &target_url, &method, body, headers).await?;
    let duration_ms = started.elapsed().as_millis() as i32;

    {
        let mut state_ref = state.borrow_mut();
        let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
        execution.checkpoints.push(FetchCheckpoint {
            call_index,
            boundary: "http".to_string(),
            url: original_url.clone(),
            method: method.clone(),
            request: request_json,
            response: response.clone(),
            duration_ms,
        });
    }

    tracing::debug!(%request_id, %call_index, original_url = %original_url, resolved_url = %target_url, "intercepted fetch");
    Ok(response)
}

/// Returns current time as milliseconds since Unix epoch.
/// In Replay mode returns the timestamp recorded during the original Live execution,
/// making `Date.now()` deterministic across replays.
#[op2(fast)]
fn op_now(state: &mut OpState) -> f64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let exec = state.borrow_mut::<RuntimeExecutionState>();
    match exec.context.mode {
        ExecutionMode::Replay => exec.recorded_now_ms.unwrap_or(now_ms) as f64,
        ExecutionMode::Live => {
            if exec.recorded_now_ms.is_none() {
                exec.recorded_now_ms = Some(now_ms);
            }
            now_ms as f64
        }
    }
}

/// Captures `console.log/warn/error` output and links it to the current execution.
/// Always prints to stdout/stderr so output is still visible during development.
#[op2(fast)]
fn op_console(state: &mut OpState, #[string] msg: String, is_err: bool) {
    let exec = state.borrow_mut::<RuntimeExecutionState>();
    exec.logs.push(LogEntry {
        level: if is_err { "error".to_string() } else { "log".to_string() },
        message: msg.clone(),
    });
    if is_err {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
}

/// Returns the effective timer delay to use.
/// In Replay mode always returns 0 so `setTimeout`/`setInterval` fire immediately,
/// keeping replay fast and avoiding real-time waits on recorded data.
#[op2(fast)]
fn op_timer_delay(state: &mut OpState, delay_ms: f64) -> f64 {
    let exec = state.borrow::<RuntimeExecutionState>();
    match exec.context.mode {
        ExecutionMode::Replay => 0.0,
        ExecutionMode::Live => delay_ms,
    }
}

/// In Live mode: generate a value via `rand`, record it for later storage.
/// In Replay mode: return the next recorded value in sequence (fallback: 0.5).
#[op2(fast)]
fn op_random(state: &mut OpState) -> f64 {
    let exec = state.borrow_mut::<RuntimeExecutionState>();
    match exec.context.mode {
        ExecutionMode::Live => {
            let v: f64 = rand::thread_rng().r#gen();
            exec.recorded_random.push(v);
            v
        }
        ExecutionMode::Replay => {
            let idx = exec.random_index;
            exec.random_index += 1;
            exec.recorded_random.get(idx).copied().unwrap_or(0.5)
        }
    }
}

/// In Live mode: generate a UUID v4 and record it.
/// In Replay mode: return the recorded UUID in sequence so `crypto.randomUUID()`
/// returns the *exact same* value as during the original execution — required
/// because user code may write the UUID to a DB and return it in the response.
#[op2]
#[string]
fn op_random_uuid(state: &mut OpState) -> String {
    let exec = state.borrow_mut::<RuntimeExecutionState>();
    match exec.context.mode {
        ExecutionMode::Live => {
            let id = Uuid::new_v4().to_string();
            exec.recorded_uuids.push(id.clone());
            id
        }
        ExecutionMode::Replay => {
            let idx = exec.uuid_index;
            exec.uuid_index += 1;
            exec.recorded_uuids
                .get(idx)
                .cloned()
                .unwrap_or_else(|| Uuid::new_v4().to_string())
        }
    }
}

/// Intercepts `Deno.serve()` in bootstrap JS — marks the isolate as a
/// long-running HTTP server.  The port hint is advisory and ignored; the Rust
/// Axum listener owns the actual TCP socket.
#[op2(fast)]
fn op_net_listen(state: &mut OpState, #[smi] _port: u32) {
    state.borrow_mut::<RuntimeExecutionState>().is_server_mode = true;
}

/// Called by the `__flux_dispatch_request` JS shim after the user handler
/// (Express/Hono/etc.) produces an HTTP response.  Stores the finalized
/// response so Rust can collect it after the event loop drains.
#[op2(fast)]
fn op_net_respond(
    state: &mut OpState,
    #[string] req_id: String,
    #[smi] status: u32,
    #[string] headers_json: String,
    #[string] body: String,
) {
    let headers: Vec<(String, String)> = serde_json::from_str::<Vec<Vec<String>>>(&headers_json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pair| {
            let mut it = pair.into_iter();
            let k = it.next()?;
            let v = it.next()?;
            Some((k, v))
        })
        .collect();
    state
        .borrow_mut::<RuntimeExecutionState>()
        .pending_responses
        .insert(req_id, NetResponse { status: status as u16, headers, body });
}

async fn make_http_request(
    client: &Client,
    url: &str,
    method: &str,
    body: Option<serde_json::Value>,
    headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let method = method
        .parse::<reqwest::Method>()
        .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

    let mut request = client.request(method, url);

    if let Some(raw_headers) = headers {
        let map: HashMap<String, String> = serde_json::from_value(raw_headers)
            .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;
        for (key, value) in map {
            request = request.header(key, value);
        }
    }

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await.map_err(|err| {
        deno_core::error::custom_error("TypeError", format!("fetch failed: {err}"))
    })?;

    // Reject responses that advertise a body larger than our limit.
    if let Some(len) = response.content_length() {
        if len as usize > MAX_RESPONSE_BYTES {
            return Err(deno_core::error::custom_error(
                "TypeError",
                format!("response too large: {len} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"),
            ));
        }
    }

    let status = response.status().as_u16();
    let response_headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            let value = v.to_str().unwrap_or_default().to_string();
            (k.to_string(), value)
        })
        .collect::<HashMap<_, _>>();

    // Stream the body with a size cap to protect against missing/lying Content-Length.
    let bytes = response
        .bytes()
        .await
        .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(deno_core::error::custom_error(
            "TypeError",
            format!(
                "response body too large: {} bytes exceeds {MAX_RESPONSE_BYTES} byte limit",
                bytes.len()
            ),
        ));
    }

    let text = String::from_utf8_lossy(&bytes).into_owned();

    let parsed_body = serde_json::from_str::<serde_json::Value>(&text)
        .unwrap_or_else(|_| serde_json::Value::String(text));

    Ok(serde_json::json!({
        "status": status,
        "headers": response_headers,
        "body": parsed_body,
    }))
}

/// Maximum V8 heap size: 128 MB.
const V8_HEAP_LIMIT: usize = 128 * 1024 * 1024;

/// Maximum execution time for a single function invocation.
const EXECUTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct JsIsolate {
    runtime: JsRuntime,
    http_client: Client,
    /// True when the user module called `Deno.serve()` during module init,
    /// meaning the isolate acts as a long-running HTTP app, not a one-shot handler.
    pub is_server_mode: bool,
}

impl JsIsolate {
    pub fn new(user_code: &str, _isolate_id: usize) -> Result<Self> {
        let http_client = Client::new();

        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init_ops_and_esm()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        // Seed OpState so that module-level ops (e.g. op_net_listen via
        // `Deno.serve()`) can run safely during `execute_script`.
        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context: ExecutionContext::new("__bootstrap__"),
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
            });
            state.put(http_client.clone());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let prepared = prepare_user_code(user_code);
        runtime
            .execute_script("flux:user_code", prepared)
            .context("failed to load user code")?;

        let is_server_mode = {
            let state = runtime.op_state();
            let state = state.borrow();
            state.borrow::<RuntimeExecutionState>().is_server_mode
        };

        Ok(Self {
            runtime,
            http_client,
            is_server_mode,
        })
    }

    /// Dispatch a single HTTP request into a server-mode isolate.  The JS
    /// `__flux_dispatch_request` shim feeds the request through the registered
    /// Hono / Express handler, which calls `op_net_respond` when done.
    pub async fn dispatch_request(
        &mut self,
        context: ExecutionContext,
        req: NetRequest,
    ) -> Result<NetResponse> {
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: true,
                pending_responses: HashMap::new(),
            });
            state.put(self.http_client.clone());
        }

        // Escape all fields into a JS expression.
        let script = format!(
            "globalThis.__flux_dispatch_request({}, {}, {}, {}, {});",
            serde_json::to_string(&req.req_id).unwrap(),
            serde_json::to_string(&req.method).unwrap(),
            serde_json::to_string(&req.url).unwrap(),
            serde_json::to_string(&req.headers_json).unwrap(),
            serde_json::to_string(&req.body).unwrap(),
        );

        self.runtime
            .execute_script("flux:dispatch", script)
            .context("failed to dispatch net request")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("server-mode request timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop failed during request dispatch")?;

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let exec = state.borrow_mut::<RuntimeExecutionState>();
        exec.pending_responses
            .remove(&req.req_id)
            .ok_or_else(|| anyhow::anyhow!("handler did not call op_net_respond for req {}", req.req_id))
    }

    pub async fn execute(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        self.execute_with_recorded(payload, context, Vec::new()).await
    }

    /// Execute with pre-recorded checkpoints injected into OpState.
    /// In Replay mode, op_fetch will return the recorded response for each call_index
    /// instead of making a live HTTP call.
    pub async fn execute_with_recorded(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
        recorded_checkpoints: Vec<FetchCheckpoint>,
    ) -> Result<JsExecutionOutput> {
        let recorded: HashMap<u32, FetchCheckpoint> = recorded_checkpoints
            .into_iter()
            .map(|cp| (cp.call_index, cp))
            .collect();
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded,
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
            });
            state.put(self.http_client.clone());
        }

        let payload_json = serde_json::to_string(&payload).context("failed to encode payload")?;
        let invoke = format!(
            "globalThis.__flux_last_result = null;\n\
             globalThis.__flux_last_error = null;\n\
             (async () => {{\n\
               try {{\n\
                 const result = await globalThis.__flux_user_handler({payload});\n\
                 globalThis.__flux_last_result = result ?? null;\n\
               }} catch (err) {{\n\
                 globalThis.__flux_last_error = String(err && err.stack ? err.stack : err);\n\
               }}\n\
             }})();",
            payload = payload_json,
        );

        self.runtime
            .execute_script("flux:invoke", invoke)
            .context("failed to invoke user handler")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("function execution timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("failed while running JS event loop")?;

        let result_value = self
            .runtime
            .execute_script(
                "flux:result",
                "JSON.stringify({ result: globalThis.__flux_last_result ?? null, error: globalThis.__flux_last_error ?? null })",
            )
            .context("failed to read handler result")?;

        let raw: String = {
            let scope = &mut self.runtime.handle_scope();
            let local = deno_core::v8::Local::new(scope, result_value);
            deno_core::serde_v8::from_v8(scope, local)
                .context("failed to deserialize handler result")?
        };

        let envelope: serde_json::Value = serde_json::from_str(&raw)
            .context("handler result envelope is not valid JSON")?;

        let (checkpoints, logs) = {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let execution = state.borrow_mut::<RuntimeExecutionState>();
            (
                std::mem::take(&mut execution.checkpoints),
                std::mem::take(&mut execution.logs),
            )
        };

        let error = envelope
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(JsExecutionOutput {
            output: envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            checkpoints,
            error,
            logs,
        })
    }
}

fn bootstrap_fetch_js() -> &'static str {
    r#"
// ── Web platform polyfills ───────────────────────────────────────────────────
// deno_core ships only V8 builtins — no fetch, Headers, Request, Response, or
// crypto. We provide minimal implementations sufficient for Deno.serve handlers
// and the standard Fetch API.

class Headers {
  #m;
  constructor(init) {
    this.#m = new Map();
    if (Array.isArray(init)) {
      for (const p of init) this.#m.set(String(p[0]).toLowerCase(), String(p[1]));
    } else if (init instanceof Headers) {
      for (const [k, v] of init.entries()) this.#m.set(k, v);
    } else if (init && typeof init === "object") {
      for (const [k, v] of Object.entries(init)) this.#m.set(k.toLowerCase(), String(v));
    }
  }
  get(n)       { return this.#m.get(String(n).toLowerCase()) ?? null; }
  set(n, v)    { this.#m.set(String(n).toLowerCase(), String(v)); }
  has(n)       { return this.#m.has(String(n).toLowerCase()); }
  delete(n)    { this.#m.delete(String(n).toLowerCase()); }
  append(n, v) {
    const k = String(n).toLowerCase();
    const cur = this.#m.get(k);
    this.#m.set(k, cur == null ? String(v) : cur + ", " + String(v));
  }
  entries()    { return this.#m.entries(); }
  keys()       { return this.#m.keys(); }
  values()     { return this.#m.values(); }
  forEach(cb)  { this.#m.forEach((v, k) => cb(v, k, this)); }
  [Symbol.iterator]() { return this.#m.entries(); }
}

class Request {
  #body;
  constructor(input, init = {}) {
    this.url    = typeof input === "string" ? input : input.url;
    this.method = ((init.method ?? (typeof input === "object" ? input.method : undefined)) ?? "GET").toUpperCase();
    this.headers = init.headers instanceof Headers
      ? init.headers
      : new Headers(init.headers);
    this.#body = init.body ?? null;
  }
  async text()        { return this.#body ?? ""; }
  async json()        { return JSON.parse(this.#body ?? "null"); }
  async arrayBuffer() {
    const s = this.#body ?? "";
    const a = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
    return a.buffer;
  }
}

class Response {
  #body;
  constructor(body, init = {}) {
    this.#body      = body == null ? "" : String(body);
    this.status     = init.status ?? 200;
    this.statusText = init.statusText ?? "";
    this.ok         = this.status >= 200 && this.status < 300;
    this.headers    = init.headers instanceof Headers
      ? init.headers
      : new Headers(init.headers);
  }
  async text() { return this.#body; }
  async json() { return JSON.parse(this.#body); }
  clone()      { return new Response(this.#body, { status: this.status, statusText: this.statusText, headers: this.headers }); }
  static json(data, init = {}) {
    const body = JSON.stringify(data);
    const h = new Headers(init.headers);
    if (!h.has("content-type")) h.set("content-type", "application/json");
    return new Response(body, { ...init, headers: h });
  }
  static error()             { return new Response("", { status: 0 }); }
  static redirect(url, s=302){ return new Response("", { status: s, headers: new Headers([["location", url]]) }); }
}

globalThis.Headers  = Headers;
globalThis.Request  = Request;
globalThis.Response = Response;

// ── URL ─────────────────────────────────────────────────────────────────────
// V8 in bare deno_core does not provide the WHATWG URL interface.
// Minimal implementation covering the properties user code typically reads.
if (!globalThis.URL) {
  globalThis.URL = class URL {
    #href; #u;
    constructor(input, base) {
      // Resolve relative URLs against base
      const str = base ? String(base).replace(/\/+$/, "") + "/" + String(input).replace(/^\/+/, "") : String(input);
      this.#href = str;
      // Parse the key components with a simple regex
      const m = str.match(/^([a-z][a-z0-9+\-.]*):\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i) || [];
      this.protocol = (m[1] ?? "").toLowerCase() + ":";
      const host = m[2] ?? "";
      const atIdx = host.lastIndexOf("@");
      const hostPart = atIdx >= 0 ? host.slice(atIdx + 1) : host;
      const portIdx = hostPart.lastIndexOf(":");
      this.hostname = portIdx >= 0 ? hostPart.slice(0, portIdx) : hostPart;
      this.port     = portIdx >= 0 ? hostPart.slice(portIdx + 1) : "";
      this.host     = hostPart;
      this.pathname = m[3] || "/";
      this.search   = m[4] ?? "";
      this.hash     = m[5] ?? "";
      this.origin   = this.protocol + "//" + this.host;
      this.href     = this.#href;
      this.searchParams = new URLSearchParams(this.search.slice(1));
    }
    toString() { return this.#href; }
  };
}

if (!globalThis.URLSearchParams) {
  globalThis.URLSearchParams = class URLSearchParams {
    #p;
    constructor(init) {
      this.#p = [];
      if (typeof init === "string") {
        for (const part of init.split("&").filter(Boolean)) {
          const [k, v = ""] = part.split("=");
          this.#p.push([decodeURIComponent(k), decodeURIComponent(v)]);
        }
      } else if (Array.isArray(init)) {
        this.#p = init.map(([k, v]) => [String(k), String(v)]);
      } else if (init && typeof init === "object") {
        for (const [k, v] of Object.entries(init)) this.#p.push([String(k), String(v)]);
      }
    }
    get(k)     { return this.#p.find(([n]) => n === k)?.[1] ?? null; }
    getAll(k)  { return this.#p.filter(([n]) => n === k).map(([,v]) => v); }
    has(k)     { return this.#p.some(([n]) => n === k); }
    set(k, v)  { this.#p = this.#p.filter(([n]) => n !== k); this.#p.push([k, String(v)]); }
    append(k, v){ this.#p.push([String(k), String(v)]); }
    delete(k)  { this.#p = this.#p.filter(([n]) => n !== k); }
    entries()  { return this.#p[Symbol.iterator](); }
    keys()     { return this.#p.map(([k]) => k)[Symbol.iterator](); }
    values()   { return this.#p.map(([,v]) => v)[Symbol.iterator](); }
    toString() { return this.#p.map(([k,v]) => encodeURIComponent(k) + "=" + encodeURIComponent(v)).join("&"); }
    forEach(cb){ this.#p.forEach(([k, v]) => cb(v, k, this)); }
  };
}

// crypto — always install our op so randomUUID() works in all contexts
if (!globalThis.crypto) globalThis.crypto = {};
globalThis.crypto.randomUUID = () => Deno.core.ops.op_random_uuid();

// ── fetch ──────────────────────────────────────────────────────────────────
globalThis.fetch = async function(url, init = {}) {
  const method = typeof init?.method === "string" ? init.method : "GET";
  const body = init?.body ?? null;
  const headers = init?.headers ?? null;
  const response = await Deno.core.ops.op_fetch(String(url), String(method), body, headers);

  return {
    status: response.status,
    ok: response.status >= 200 && response.status < 400,
    headers: response.headers ?? {},
    async json() { return response.body; },
    async text() {
      if (typeof response.body === "string") return response.body;
      return JSON.stringify(response.body ?? null);
    },
  };
};

// ── Date.now() ─────────────────────────────────────────────────────────────
// In Replay mode op_now returns the timestamp recorded during the original
// execution, making time deterministic without touching the system clock.
Date.now = function() { return Deno.core.ops.op_now(); };

// ── console ────────────────────────────────────────────────────────────────
// Capture all console output and link it to the current execution_id so
// `flux trace` can show logs alongside spans and DB mutations.
function _flux_fmt(...args) {
  return args.map(v => (typeof v === "string" ? v : JSON.stringify(v))).join(" ");
}
console.log   = (...a) => Deno.core.ops.op_console(_flux_fmt(...a), false);
console.info  = (...a) => Deno.core.ops.op_console(_flux_fmt(...a), false);
console.warn  = (...a) => Deno.core.ops.op_console(_flux_fmt(...a), false);
console.error = (...a) => Deno.core.ops.op_console(_flux_fmt(...a), true);
console.debug = (...a) => Deno.core.ops.op_console(_flux_fmt(...a), false);

// ── setTimeout / setInterval ────────────────────────────────────────────────
// In Replay mode op_timer_delay returns 0 so timers fire immediately instead
// of waiting real wall-clock time — keeps replay fast and deterministic.
const _origSetTimeout  = globalThis.setTimeout;
const _origSetInterval = globalThis.setInterval;
globalThis.setTimeout  = (fn, delay, ...args) =>
  _origSetTimeout(fn,  Deno.core.ops.op_timer_delay(delay ?? 0), ...args);
globalThis.setInterval = (fn, delay, ...args) =>
  _origSetInterval(fn, Deno.core.ops.op_timer_delay(delay ?? 0), ...args);

// ── Math.random ─────────────────────────────────────────────────────────────
// In Live mode op_random generates a real random f64 and records it.
// In Replay mode it returns the recorded value in sequence → same code path,
// same branching, deterministic execution.
Math.random = () => Deno.core.ops.op_random();

// ── Deno.serve (server mode) ─────────────────────────────────────────────────
// Intercepts `Deno.serve(handler)` so frameworks like Hono work inside Flux.
// The handler is stored globally; Rust feeds requests in via
// `__flux_dispatch_request` instead of binding a real TCP socket.
globalThis.__flux_net_handler = null;

Deno.serve = function(handlerOrOptions) {
  let handler;
  if (typeof handlerOrOptions === "function") {
    handler = handlerOrOptions;
  } else if (handlerOrOptions && typeof handlerOrOptions.fetch === "function") {
    // Hono passes { fetch } object
    handler = handlerOrOptions.fetch.bind(handlerOrOptions);
  }
  if (!handler) throw new TypeError("Deno.serve: expected a handler function or { fetch } object");

  globalThis.__flux_net_handler = handler;
  Deno.core.ops.op_net_listen(0);

  // Return a stub Deno.Server — frameworks may call .finished, .ref, .unref
  return { ref() {}, unref() {}, shutdown() {}, finished: Promise.resolve() };
};

// Called by Rust (via execute_script) for each incoming HTTP request.
globalThis.__flux_dispatch_request = async function(reqId, method, url, headersJson, body) {
  const handler = globalThis.__flux_net_handler;
  if (!handler) {
    Deno.core.ops.op_net_respond(reqId, 500, "[]", "No Deno.serve handler registered");
    return;
  }

  let headersInit;
  try {
    // headersJson is [[name,value],...] from Rust
    headersInit = JSON.parse(headersJson);
  } catch {
    headersInit = [];
  }

  const request = new Request(url, {
    method,
    headers: new Headers(headersInit),
    body: (method === "GET" || method === "HEAD") ? undefined : (body || undefined),
  });

  let response;
  try {
    response = await handler(request);
  } catch (err) {
    const msg = String(err && err.stack ? err.stack : err);
    Deno.core.ops.op_net_respond(reqId, 500, "[]", msg);
    return;
  }

  let responseBody;
  try { responseBody = await response.text(); } catch { responseBody = ""; }

  const responseHeaders = JSON.stringify([...response.headers.entries()]);
  Deno.core.ops.op_net_respond(reqId, response.status, responseHeaders, responseBody);
};
"#
}

fn prepare_user_code(code: &str) -> String {
    let transformed = if code.contains("export default async function") {
        code.replacen(
            "export default async function",
            "globalThis.__flux_user_handler = async function",
            1,
        )
    } else if code.contains("export default function") {
        code.replacen(
            "export default function",
            "globalThis.__flux_user_handler = function",
            1,
        )
    } else if code.contains("export default") {
        code.replacen("export default", "globalThis.__flux_user_handler =", 1)
    } else {
        code.to_string()
    };

    // In server mode (Deno.serve was called) __flux_net_handler is set instead
    // of __flux_user_handler — skip the export guard in that case.
    format!(
        "{}\n\
         if (typeof globalThis.__flux_net_handler !== 'function' && \
             typeof globalThis.__flux_user_handler !== 'function') {{\n\
           throw new Error('entry module must export default function or call Deno.serve()');\n\
         }}",
        transformed
    )
}
