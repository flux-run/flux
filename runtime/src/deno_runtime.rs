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

/// Per-isolate map of in-flight execution states, keyed by execution_id.
/// Stored once in `OpState`; each concurrent execution owns its own slot.
type RuntimeStateMap = HashMap<String, RuntimeExecutionState>;

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

deno_core::extension!(flux_runtime_ext, ops = [
    op_begin_execution,
    op_end_execution,
    op_fetch,
    op_now,
    op_console,
    op_timer_delay,
    op_random,
    op_random_uuid,
    op_net_listen,
    op_net_respond,
]);

/// Called by JS at the start of every execution to register a state slot.
/// `recorded_random_json` and `recorded_uuids_json` are JSON-encoded arrays for
/// replay mode; pass `"[]"` for live executions.
#[op2(fast)]
fn op_begin_execution(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] request_id: String,
    #[string] code_version: String,
    is_replay: bool,
    #[string] recorded_random_json: String,
    #[string] recorded_uuids_json: String,
    #[string] recorded_now_ms_json: String,
) {
    let recorded_random: Vec<f64> =
        serde_json::from_str(&recorded_random_json).unwrap_or_default();
    let recorded_uuids: Vec<String> =
        serde_json::from_str(&recorded_uuids_json).unwrap_or_default();
    let recorded_now_ms: Option<u64> =
        serde_json::from_str(&recorded_now_ms_json).unwrap_or(None);

    let exec_state = RuntimeExecutionState {
        context: ExecutionContext {
            execution_id: execution_id.clone(),
            request_id,
            code_version,
            mode: if is_replay { ExecutionMode::Replay } else { ExecutionMode::Live },
        },
        call_index: 0,
        checkpoints: Vec::new(),
        recorded: HashMap::new(),
        recorded_now_ms,
        logs: Vec::new(),
        recorded_random,
        random_index: 0,
        recorded_uuids,
        uuid_index: 0,
        is_server_mode: false,
        pending_responses: HashMap::new(),
    };

    state
        .borrow_mut::<RuntimeStateMap>()
        .insert(execution_id, exec_state);
}

/// Called by JS at the end of every execution.  Returns a JSON string with the
/// collected checkpoints, logs, random values, and uuids so Rust can harvest
/// them without an extra op round-trip.
#[op2]
#[string]
fn op_end_execution(state: &mut OpState, #[string] execution_id: String) -> String {
    let slot = state
        .borrow_mut::<RuntimeStateMap>()
        .remove(&execution_id);

    match slot {
        Some(s) => serde_json::to_string(&serde_json::json!({
            "checkpoints": s.checkpoints,
            "logs":        s.logs,
            "random":      s.recorded_random,
            "uuids":       s.recorded_uuids,
            "now_ms":      s.recorded_now_ms,
        }))
        .unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

#[op2(async)]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] execution_id: String,
    #[string] url: String,
    #[string] method: String,
    #[serde] body: Option<serde_json::Value>,
    #[serde] headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let original_url = url;

    let (request_id, call_index, mode, recorded_checkpoint, client) = {
        let mut state_ref = state.borrow_mut();
        let (request_id, index, mode, recorded) = {
            let map = state_ref.borrow_mut::<RuntimeStateMap>();
            let execution = map.get_mut(&execution_id).ok_or_else(|| {
                deno_core::error::custom_error(
                    "InternalError",
                    format!("op_fetch: execution_id '{execution_id}' not found"),
                )
            })?;
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
                let map = state_ref.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, "replay: returned recorded response");
            return Ok(response);
        }
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
        let map = state_ref.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
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
    }

    tracing::debug!(%request_id, %call_index, original_url = %original_url, resolved_url = %target_url, "intercepted fetch");
    Ok(response)
}

/// Returns current time as milliseconds since Unix epoch.
/// In Replay mode returns the timestamp recorded during the original Live execution,
/// making `Date.now()` deterministic across replays.
#[op2(fast)]
fn op_now(state: &mut OpState, #[string] execution_id: String) -> f64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return now_ms as f64,
    };

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
#[op2(fast)]
fn op_console(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] msg: String,
    is_err: bool,
) {
    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.logs.push(LogEntry {
            level: if is_err { "error".to_string() } else { "log".to_string() },
            message: msg.clone(),
        });
    }
    if is_err {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
}

/// Returns the effective timer delay to use.
/// In Replay mode always returns 0 so timers fire immediately.
#[op2(fast)]
fn op_timer_delay(state: &mut OpState, #[string] execution_id: String, delay_ms: f64) -> f64 {
    let map = state.borrow_mut::<RuntimeStateMap>();
    match map.get(&execution_id) {
        Some(exec) => match exec.context.mode {
            ExecutionMode::Replay => 0.0,
            ExecutionMode::Live => delay_ms,
        },
        None => delay_ms,
    }
}

/// In Live mode: generate a value via `rand`, record it for later storage.
/// In Replay mode: return the next recorded value in sequence (fallback: 0.5).
#[op2(fast)]
fn op_random(state: &mut OpState, #[string] execution_id: String) -> f64 {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return rand::thread_rng().r#gen(),
    };
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
/// In Replay mode: return the recorded UUID in sequence.
#[op2]
#[string]
fn op_random_uuid(state: &mut OpState, #[string] execution_id: String) -> String {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return Uuid::new_v4().to_string(),
    };
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

/// Intercepts `Deno.serve()` — marks the isolate as a long-running HTTP server.
#[op2(fast)]
fn op_net_listen(state: &mut OpState, #[string] execution_id: String, #[smi] _port: u32) {
    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.is_server_mode = true;
    }
}

/// Called by the `__flux_dispatch_request` JS shim after the handler produces
/// an HTTP response.  Stores the finalized response keyed by req_id.
#[op2(fast)]
fn op_net_respond(
    state: &mut OpState,
    #[string] execution_id: String,
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

    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.pending_responses
            .insert(req_id, NetResponse { status: status as u16, headers, body });
    }
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
        Self::new_internal(user_code, prepare_user_code(user_code))
    }

    /// Variant used by `flux run` / `--script-mode`.  Accepts plain top-level
    /// scripts (no `export default` required) while still wiring up the handler
    /// global when `export default` IS present.
    pub fn new_for_run(user_code: &str) -> Result<Self> {
        Self::new_internal(user_code, prepare_run_code(user_code))
    }

    fn new_internal(_user_code: &str, prepared: String) -> Result<Self> {
        let http_client = Client::new();

        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init_ops_and_esm()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        // Seed OpState with an empty execution-state map and the HTTP client.
        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put::<RuntimeStateMap>(HashMap::new());
            state.put(http_client.clone());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        runtime
            .execute_script("flux:user_code", prepared)
            .context("failed to load user code")?;

        // Check if the module called Deno.serve() during init.
        // In the new model, server-mode detection uses a bootstrap execution slot.
        let is_server_mode = {
            let state = runtime.op_state();
            let state = state.borrow();
            // Deno.serve wires up __flux_net_handler; check for it instead of OpState.
            // (no state slot exists yet — we check the JS side via a script)
            drop(state);
            let probe = runtime
                .execute_script(
                    "flux:probe_server_mode",
                    "typeof globalThis.__flux_net_handler === 'function'",
                )
                .context("failed to probe server mode")?;
            let scope = &mut runtime.handle_scope();
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
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
        let execution_id = context.execution_id.clone();
        let request_id = context.request_id.clone();

        // Register a state slot for this request.
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
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

        // Inject execution_id so the JS shim can thread it through all ops.
        let script = format!(
            "globalThis.__FLUX_EXECUTION_ID__ = {};\n\
             globalThis.__flux_dispatch_request({}, {}, {}, {}, {});",
            serde_json::to_string(&execution_id).unwrap(),
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
        let map = state.borrow_mut::<RuntimeStateMap>();
        let exec = map
            .remove(&execution_id)
            .ok_or_else(|| anyhow::anyhow!("state slot missing for execution {execution_id}"))?;
        exec.pending_responses
            .into_values()
            .next()
            .ok_or_else(|| anyhow::anyhow!("handler did not call op_net_respond for req {} (request_id={})", req.req_id, request_id))
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
        let execution_id = context.execution_id.clone();
        let recorded: HashMap<u32, FetchCheckpoint> = recorded_checkpoints
            .into_iter()
            .map(|cp| (cp.call_index, cp))
            .collect();

        // Register the state slot before injecting JS.
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
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

        let eid_json = serde_json::to_string(&execution_id).context("failed to encode execution_id")?;
        let payload_json = serde_json::to_string(&payload).context("failed to encode payload")?;
        let invoke = format!(
            "(async () => {{\n\
               const __eid = {eid};\n\
               globalThis.__FLUX_EXECUTION_ID__ = __eid;\n\
               globalThis.__flux_last_result = globalThis.__flux_last_result || {{}};\n\
               globalThis.__flux_last_result[__eid] = null;\n\
               globalThis.__flux_last_error = globalThis.__flux_last_error || {{}};\n\
               globalThis.__flux_last_error[__eid] = null;\n\
               try {{\n\
                 const ctx = {{}};\n\
                 const result = await globalThis.__flux_user_handler({{ input: {payload}, ctx }});\n\
                 globalThis.__flux_last_result[__eid] = result ?? null;\n\
               }} catch (err) {{\n\
                 globalThis.__flux_last_error[__eid] = String(err && err.stack ? err.stack : err);\n\
               }}\n\
             }})();",
            eid = eid_json,
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

        let result_script = format!(
            "JSON.stringify({{ result: (globalThis.__flux_last_result || {{}})[{eid}] ?? null, error: (globalThis.__flux_last_error || {{}})[{eid}] ?? null }})",
            eid = eid_json,
        );

        let result_value = self
            .runtime
            .execute_script("flux:result", result_script)
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
            let map = state.borrow_mut::<RuntimeStateMap>();
            match map.remove(&execution_id) {
                Some(execution) => (execution.checkpoints, execution.logs),
                None => (Vec::new(), Vec::new()),
            }
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

    /// Run the entry module in script mode — the `flux run` equivalent of
    /// `node script.js`.
    ///
    /// Two sub-modes, selected automatically:
    ///
    /// **Handler mode** — the file exports a default function.  Flux calls it
    /// with `input` (defaults to `{}`), drains the event loop, and returns the
    /// output and any captured logs.
    ///
    /// **Top-level mode** — no exported handler.  `input` is ignored.  Flux
    /// simply drains the event loop so that top-level `await` and `setTimeout`
    /// promises resolve, then returns the captured logs.
    ///
    /// In both cases, `console.log/warn/error` output is streamed to
    /// stdout/stderr by `op_console` AND collected in the returned log vec.
    pub async fn run_script(&mut self, input: serde_json::Value) -> Result<(Option<serde_json::Value>, Vec<LogEntry>)> {
        // Check whether the module registered a handler during initialisation.
        let has_handler = {
            let check = self.runtime
                .execute_script(
                    "flux:check_handler",
                    "typeof globalThis.__flux_user_handler === 'function'",
                )
                .context("failed to check for exported handler")?;
            let scope = &mut self.runtime.handle_scope();
            let local = deno_core::v8::Local::new(scope, check);
            local.is_true()
        };

        if has_handler {
            let context = ExecutionContext::new("__run__");
            let output = self.execute(input, context).await?;
            if let Some(ref err) = output.error {
                eprintln!("error: {err}");
            }
            return Ok((Some(output.output), output.logs));
        }

        // Top-level mode: register a transient state slot so ops don't panic,
        // then drain the event loop.
        let execution_id = "__script__".to_string();
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
                context: ExecutionContext::new("__script__"),
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
        }

        // Tell bootstrap JS which execution_id to use for top-level ops.
        let eid_json = serde_json::to_string(&execution_id).unwrap();
        self.runtime
            .execute_script("flux:set_script_eid", format!("globalThis.__FLUX_EXECUTION_ID__ = {eid_json};"))
            .context("failed to set execution_id")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("script timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during script execution")?;

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let logs = map
            .remove(&execution_id)
            .map(|e| e.logs)
            .unwrap_or_default();
        Ok((None, logs))
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
if (!globalThis.URL) {
  globalThis.URL = class URL {
    #href; #u;
    constructor(input, base) {
      const str = base ? String(base).replace(/\/+$/, "") + "/" + String(input).replace(/^\/+/, "") : String(input);
      this.#href = str;
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

// ── Execution ID accessor ────────────────────────────────────────────────────
// All op wrappers below use this helper so each concurrent execution threads
// its own ID through the Rust ops, which index into the per-execution HashMap.
function __flux_eid() {
  return globalThis.__FLUX_EXECUTION_ID__ || "__unknown__";
}

// ── crypto ──────────────────────────────────────────────────────────────────
if (!globalThis.crypto) globalThis.crypto = {};
globalThis.crypto.randomUUID = () => Deno.core.ops.op_random_uuid(__flux_eid());

// ── fetch ──────────────────────────────────────────────────────────────────
globalThis.fetch = async function(url, init = {}) {
  const method = typeof init?.method === "string" ? init.method : "GET";
  const body = init?.body ?? null;
  const headers = init?.headers ?? null;
  const response = await Deno.core.ops.op_fetch(__flux_eid(), String(url), String(method), body, headers);

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

// ── Date.now() + new Date() ────────────────────────────────────────────────
{
  const _OrigDate = globalThis.Date;
  class PatchedDate extends _OrigDate {
    constructor(...args) {
      if (args.length === 0) {
        super(Deno.core.ops.op_now(__flux_eid()));
      } else {
        super(...args);
      }
    }
  }
  PatchedDate.now = function() { return Deno.core.ops.op_now(__flux_eid()); };
  globalThis.Date = PatchedDate;
}

// ── performance.now() ──────────────────────────────────────────────────────
if (globalThis.performance) {
  globalThis.performance.now = function() { return Deno.core.ops.op_now(__flux_eid()); };
}

// ── console ────────────────────────────────────────────────────────────────
function _flux_fmt(...args) {
  return args.map(v => (typeof v === "string" ? v : JSON.stringify(v))).join(" ");
}
console.log   = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.info  = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.warn  = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.error = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), true);
console.debug = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);

// ── setTimeout / setInterval ────────────────────────────────────────────────
const _origSetTimeout  = globalThis.setTimeout;
const _origSetInterval = globalThis.setInterval;
globalThis.setTimeout  = (fn, delay, ...args) =>
  _origSetTimeout(fn,  Deno.core.ops.op_timer_delay(__flux_eid(), delay ?? 0), ...args);
globalThis.setInterval = (fn, delay, ...args) =>
  _origSetInterval(fn, Deno.core.ops.op_timer_delay(__flux_eid(), delay ?? 0), ...args);

// ── Math.random ─────────────────────────────────────────────────────────────
Math.random = () => Deno.core.ops.op_random(__flux_eid());

// ── Deno.serve (server mode) ─────────────────────────────────────────────────
globalThis.__flux_net_handler = null;

Deno.serve = function(handlerOrOptions) {
  let handler;
  if (typeof handlerOrOptions === "function") {
    handler = handlerOrOptions;
  } else if (handlerOrOptions && typeof handlerOrOptions.fetch === "function") {
    handler = handlerOrOptions.fetch.bind(handlerOrOptions);
  }
  if (!handler) throw new TypeError("Deno.serve: expected a handler function or { fetch } object");

  globalThis.__flux_net_handler = handler;
  Deno.core.ops.op_net_listen(__flux_eid(), 0);

  return { ref() {}, unref() {}, shutdown() {}, finished: Promise.resolve() };
};

// Called by Rust (via execute_script) for each incoming HTTP request.
globalThis.__flux_dispatch_request = async function(reqId, method, url, headersJson, body) {
  const __eid = globalThis.__FLUX_EXECUTION_ID__;
  const handler = globalThis.__flux_net_handler;
  if (!handler) {
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", "No Deno.serve handler registered");
    return;
  }

  let headersInit;
  try {
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
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", msg);
    return;
  }

  let responseBody;
  try { responseBody = await response.text(); } catch { responseBody = ""; }

  const responseHeaders = JSON.stringify([...response.headers.entries()]);
  Deno.core.ops.op_net_respond(__eid, reqId, response.status ?? 200, responseHeaders, responseBody);
};
"#
}

fn prepare_user_code(code: &str) -> String {
    let transformed = rewrite_export_default(code);

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

/// Like `prepare_user_code` but without the mandatory-export guard, so plain
/// top-level scripts (no `export default`) can run without throwing.
/// Used exclusively by the `flux run` / `--script-mode` path.
fn prepare_run_code(code: &str) -> String {
    rewrite_export_default(code)
}

/// Rewrite `export default [async] function` / `export default <expr>` into
/// `globalThis.__flux_user_handler = …` so the Rust host can invoke the
/// handler without ES module machinery.
fn rewrite_export_default(code: &str) -> String {
    if code.contains("export default async function") {
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
    }
}
