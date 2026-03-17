use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::Path;
use std::rc::Rc;

use anyhow::{Context, Result};
use deno_ast::{EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions, TranspileOptions};
use deno_core::{JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, ResolutionKind, RuntimeOptions, op2, resolve_import, resolve_path};
use deno_error::JsErrorBox;
use rand::Rng;
use serde::{Deserialize, Serialize};
use ureq::OrAnyStatus;
use uuid::Uuid;

use crate::isolate_pool::ExecutionContext;

/// Per-isolate map of in-flight execution states, keyed by execution_id.
/// Stored once in `OpState`; each concurrent execution owns its own slot.
type RuntimeStateMap = HashMap<String, RuntimeExecutionState>;

/// Maximum response body size: 10 MB.
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Maximum redirect hops for a single buffered fetch.
const MAX_REDIRECTS: usize = 5;

/// Blocked metadata hostnames (cloud provider instance metadata endpoints).
const BLOCKED_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "169.254.170.2",
];

/// Validate that a URL is safe to fetch — blocks SSRF to cloud metadata and private IPs.
fn validate_fetch_url(raw_url: &str) -> std::result::Result<(), JsErrorBox> {
    let parsed = url::Url::parse(raw_url)
        .map_err(|e| JsErrorBox::type_error(format!("invalid URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("invalid URL: no host"))?;

    for blocked in BLOCKED_HOSTS {
        if host == *blocked {
            return Err(JsErrorBox::generic(format!(
                "fetch blocked: {host} is a restricted endpoint"
            )));
        }
    }

    let allow_loopback = std::env::var("FLOWBASE_ALLOW_LOOPBACK_FETCH")
        .map(|value| value == "1")
        .unwrap_or(false);

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_ip_addr(&ip, allow_loopback)?;
    } else if let Some(port) = parsed.port_or_known_default() {
        if let Ok(resolved) = (host, port).to_socket_addrs() {
            for addr in resolved {
                validate_ip_addr(&addr.ip(), allow_loopback)?;
            }
        }
    }

    Ok(())
}

fn validate_ip_addr(ip: &IpAddr, allow_loopback: bool) -> std::result::Result<(), JsErrorBox> {
    if ip.is_loopback() {
        if allow_loopback {
            return Ok(());
        }
        return Err(JsErrorBox::new(
            "Error",
            "fetch blocked: private/loopback IP addresses are not allowed",
        ));
    }

    if is_private_ip(ip) {
        return Err(JsErrorBox::new(
            "Error",
            "fetch blocked: private/loopback IP addresses are not allowed",
        ));
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
            (v6.segments()[0] & 0xfe00) == 0xfc00  // fc00::/7 unique-local
            || (v6.segments()[0] & 0xffc0) == 0xfe80  // fe80::/10 link-local
        }
    }
}

fn normalized_headers_value(headers: &HashMap<String, String>) -> serde_json::Value {
    let normalized: BTreeMap<String, String> = headers
        .iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
        .collect();
    serde_json::to_value(normalized).unwrap_or_else(|_| serde_json::json!({}))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchRequestPayload {
    execution_id: String,
    url: String,
    method: String,
    body: Option<String>,
    headers: HashMap<String, String>,
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
    op_flux_fetch,
    op_flux_now,
    op_flux_parse_url,
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

#[op2]
#[serde]
fn op_flux_fetch(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let FetchRequestPayload {
        execution_id,
        url: original_url,
        method,
        body,
        headers,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid fetch request: {err}")))?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let (request_id, index, mode, recorded) = {
            let map = state.borrow_mut::<RuntimeStateMap>();
            let execution = map.get_mut(&execution_id).ok_or_else(|| {
                JsErrorBox::new(
                    "InternalError",
                    format!("op_flux_fetch: execution_id '{execution_id}' not found"),
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
        (request_id, index, mode, recorded)
    };

    // SSRF protection applies to live and replay paths. Recorded checkpoints must
    // never bypass the runtime's URL safety gate.
    validate_fetch_url(&original_url)?;

    // In Replay mode, return the recorded response instead of making a live call.
    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
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

    let resolved_url = original_url.clone();
    let request_json = serde_json::json!({
        "url": original_url.clone(),
        "resolved_url": resolved_url.clone(),
        "method": method.clone(),
        "body": body.clone(),
        "headers": normalized_headers_value(&headers),
    });

    let started = std::time::Instant::now();
    let target_url = resolved_url;

    let response = make_http_request(&target_url, &method, body.clone(), Some(headers.clone()))?;
    let duration_ms = started.elapsed().as_millis() as i32;

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
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
fn op_flux_now(state: &mut OpState, #[string] execution_id: String) -> f64 {
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

#[op2]
#[serde]
fn op_flux_parse_url(
    #[string] input: String,
    #[string] base: String,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    let parsed = if base.is_empty() {
        url::Url::parse(&input)
    } else {
        let base_url = url::Url::parse(&base)
            .map_err(|err| JsErrorBox::type_error(format!("invalid base URL: {err}")))?;
        base_url.join(&input)
    }
    .map_err(|err| JsErrorBox::type_error(format!("invalid URL: {err}")))?;

    Ok(serde_json::json!({
        "href": parsed.as_str(),
        "origin": parsed.origin().ascii_serialization(),
        "protocol": format!("{}:", parsed.scheme()),
        "username": parsed.username(),
        "password": parsed.password(),
        "host": parsed.host_str().map(|host| {
            parsed.port().map(|port| format!("{host}:{port}")).unwrap_or_else(|| host.to_string())
        }).unwrap_or_default(),
        "hostname": parsed.host_str().unwrap_or_default(),
        "port": parsed.port().map(|port| port.to_string()).unwrap_or_default(),
        "pathname": parsed.path(),
        "search": parsed.query().map(|query| format!("?{query}")).unwrap_or_default(),
        "hash": parsed.fragment().map(|fragment| format!("#{fragment}")).unwrap_or_default(),
    }))
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

fn make_http_request(
    url: &str,
    method: &str,
    body: Option<String>,
    headers: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, JsErrorBox> {
    let agent = ureq::builder().redirects(0).build();
    let request_headers = headers.unwrap_or_default();
    let mut current_url = url.to_string();
    let mut current_method = method.to_string();
    let mut current_body = body;
    let response = {
        let mut final_response = None;
        for redirect_count in 0..=MAX_REDIRECTS {
        validate_fetch_url(&current_url)?;

        let mut request = agent.request(&current_method, &current_url);
        for (key, value) in &request_headers {
            request = request.set(key, value);
        }

        let response = match current_body.as_deref() {
            Some(body) => request.send_string(body),
            None => request.call(),
        }
        .or_any_status()
        .map_err(|err| JsErrorBox::type_error(format!("fetch failed: {err}")))?;

        match response.status() {
            301 | 302 | 303 | 307 | 308 => {
                if redirect_count == MAX_REDIRECTS {
                    return Err(JsErrorBox::type_error("too many redirects"));
                }

                let location = response.header("location").ok_or_else(|| {
                    JsErrorBox::type_error("redirect response missing Location header")
                })?;

                let next_url = url::Url::parse(&current_url)
                    .and_then(|base| base.join(location))
                    .map_err(|err| JsErrorBox::type_error(format!("invalid redirect URL: {err}")))?
                    .to_string();

                if current_url == next_url {
                    return Err(JsErrorBox::type_error("redirect loop detected"));
                }

                match response.status() {
                    301 | 302 if current_method.eq_ignore_ascii_case("POST") => {
                        current_method = "GET".to_string();
                        current_body = None;
                    }
                    303 if !current_method.eq_ignore_ascii_case("HEAD") => {
                        current_method = "GET".to_string();
                        current_body = None;
                    }
                    _ => {}
                }

                current_url = next_url;
                continue;
            }
            _ => {
                final_response = Some(response);
                break;
            }
        }
        }
        final_response.ok_or_else(|| JsErrorBox::type_error("redirect resolution failed"))?
    };

    // Reject responses that advertise a body larger than our limit.
    if let Some(len) = response
        .header("content-length")
        .and_then(|value: &str| value.parse::<usize>().ok())
    {
        if len > MAX_RESPONSE_BYTES {
            return Err(JsErrorBox::type_error(
                format!("response too large: {len} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"),
            ));
        }
    }

    let status = response.status();
    let response_headers: BTreeMap<String, String> = response
        .headers_names()
        .into_iter()
        .filter_map(|name: String| {
            response
                .header(&name)
                .map(|value: &str| (name.to_ascii_lowercase(), value.to_string()))
        })
        .collect();

    // Stream the body with a size cap to protect against missing/lying Content-Length.
    let reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err: std::io::Error| JsErrorBox::type_error(err.to_string()))?;

    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(JsErrorBox::type_error(
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

type SourceMapStore = Rc<RefCell<HashMap<String, Vec<u8>>>>;

struct TypescriptModuleLoader {
    source_maps: SourceMapStore,
}

fn should_transpile_media_type(media_type: MediaType) -> bool {
    matches!(
        media_type,
        MediaType::Jsx
            | MediaType::TypeScript
            | MediaType::Mts
            | MediaType::Cts
            | MediaType::Dts
            | MediaType::Dmts
            | MediaType::Dcts
            | MediaType::Tsx
    )
}

fn transpile_module_source(
    module_specifier: &ModuleSpecifier,
    media_type: MediaType,
    source: String,
    source_maps: Option<&SourceMapStore>,
) -> std::result::Result<String, JsErrorBox> {
    if !should_transpile_media_type(media_type) {
        return Ok(source);
    }

    let result = deno_ast::parse_module(ParseParams {
        specifier: module_specifier.clone(),
        text: source.into(),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(JsErrorBox::from_err)?
    .transpile(
        &TranspileOptions::default(),
        &TranspileModuleOptions::default(),
        &EmitOptions {
            source_map: SourceMapOption::Separate,
            inline_sources: true,
            ..Default::default()
        },
    )
    .map_err(JsErrorBox::from_err)?
    .into_source();

    if let (Some(source_maps), Some(source_map)) = (source_maps, result.source_map) {
        source_maps
            .borrow_mut()
            .insert(module_specifier.to_string(), source_map.into_bytes());
    }

    Ok(result.text)
}

impl ModuleLoader for TypescriptModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> std::result::Result<ModuleSpecifier, deno_core::error::ModuleLoaderError> {
        resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let source_maps = self.source_maps.clone();
        let module_specifier = module_specifier.clone();

        fn load_module(
            source_maps: SourceMapStore,
            module_specifier: &ModuleSpecifier,
            options: &ModuleLoadOptions,
        ) -> std::result::Result<ModuleSource, deno_core::error::ModuleLoaderError> {
            let path = module_specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic("Only file:// URLs are supported."))?;

            let media_type = MediaType::from_path(&path);
            let module_type = match media_type {
                MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs => {
                    ModuleType::JavaScript
                }
                MediaType::TypeScript
                | MediaType::Mts
                | MediaType::Cts
                | MediaType::Dts
                | MediaType::Dmts
                | MediaType::Dcts
                | MediaType::Tsx
                | MediaType::Jsx => ModuleType::JavaScript,
                MediaType::Json => ModuleType::Json,
                _ => {
                    return Err(JsErrorBox::generic(format!(
                        "unsupported module extension: {}",
                        path.display()
                    )));
                }
            };

            if module_type == ModuleType::Json
                && options.requested_module_type != deno_core::RequestedModuleType::Json
            {
                return Err(JsErrorBox::generic(
                    "attempted to load JSON module without `with { type: \"json\" }`",
                ));
            }

            let source = std::fs::read_to_string(&path)
                .map_err(JsErrorBox::from_err)?;
            let source = transpile_module_source(
                module_specifier,
                media_type,
                source,
                Some(&source_maps),
            )?;

            Ok(ModuleSource::new(
                module_type,
                ModuleSourceCode::String(source.into()),
                module_specifier,
                None,
            ))
        }

        ModuleLoadResponse::Sync(load_module(source_maps, &module_specifier, &options))
    }

    fn get_source_map(&self, specifier: &str) -> Option<Cow<'_, [u8]>> {
        self.source_maps
            .borrow()
            .get(specifier)
            .map(|value| value.clone().into())
    }
}

pub struct JsIsolate {
    runtime: JsRuntime,
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

    /// Variant used by `flux run` when loading a real JS/TS module entry.
    /// Supports relative ESM imports and TypeScript transpilation on demand.
    pub async fn new_for_run_entry(entry: &Path) -> Result<Self> {
        let source_maps = Rc::new(RefCell::new(HashMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(Rc::new(TypescriptModuleLoader {
                source_maps,
            })),
            extensions: vec![flux_runtime_ext::init()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put::<RuntimeStateMap>(HashMap::new());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let main_module = resolve_path(
            entry.to_str().ok_or_else(|| anyhow::anyhow!("invalid entry path: {}", entry.display()))?,
            &std::env::current_dir().context("failed to get current working directory")?,
        )
        .with_context(|| format!("failed to resolve module specifier for {}", entry.display()))?;

        let entry_source = std::fs::read_to_string(entry)
            .with_context(|| format!("failed to read {}", entry.display()))?;
        let transformed_entry = prepare_run_code(&entry_source);
        let transformed_entry = transpile_module_source(
            &main_module,
            MediaType::from_path(entry),
            transformed_entry,
            None,
        )
        .context("failed to transpile entry module")?;

        let module_id = runtime
            .load_main_es_module_from_code(&main_module, transformed_entry)
            .await
            .context("failed to load user module")?;
        let evaluation = runtime.mod_evaluate(module_id);
        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during module initialization")?;
        evaluation
            .await
            .context("failed to evaluate user module")?;

        let is_server_mode = {
            let probe = runtime
                .execute_script(
                    "flux:probe_server_mode",
                    "typeof globalThis.__flux_net_handler === 'function'",
                )
                .context("failed to probe server mode")?;
            deno_core::scope!(scope, &mut runtime);
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
        };

        Ok(Self { runtime, is_server_mode })
    }

    fn new_internal(_user_code: &str, prepared: String) -> Result<Self> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init()],
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
            deno_core::scope!(scope, &mut runtime);
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
        };

        Ok(Self { runtime, is_server_mode })
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
        self
            .execute_handler_with_recorded(payload, context, recorded_checkpoints, true)
            .await
    }

    async fn execute_handler_with_recorded(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
        recorded_checkpoints: Vec<FetchCheckpoint>,
        wrap_payload_in_input: bool,
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
        }

        let eid_json = serde_json::to_string(&execution_id).context("failed to encode execution_id")?;
        let payload_json = serde_json::to_string(&payload).context("failed to encode payload")?;
        let handler_arg = if wrap_payload_in_input {
            format!("{{ input: {payload_json}, ctx }}")
        } else {
            payload_json.clone()
        };
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
                                 const result = await globalThis.__flux_user_handler({handler_arg});\n\
                 globalThis.__flux_last_result[__eid] = result ?? null;\n\
               }} catch (err) {{\n\
                 globalThis.__flux_last_error[__eid] = String(err && err.stack ? err.stack : err);\n\
               }}\n\
             }})();",
            eid = eid_json,
                        handler_arg = handler_arg,
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
            deno_core::scope!(scope, &mut self.runtime);
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
            deno_core::scope!(scope, &mut self.runtime);
            let local = deno_core::v8::Local::new(scope, check);
            local.is_true()
        };

        if has_handler {
            let context = ExecutionContext::new("__run__");
            let output = self
                .execute_handler_with_recorded(input, context, Vec::new(), false)
                .await?;
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
// Flux provides a small buffered web-platform shim here. Transport, time,
// randomness, logging, and server dispatch remain owned by Flux so recording
// and replay stay deterministic.

// ── Execution ID accessor ────────────────────────────────────────────────────
// All op wrappers below use this helper so each concurrent execution threads
// its own ID through the Rust ops, which index into the per-execution HashMap.
function __flux_eid() {
  return globalThis.__FLUX_EXECUTION_ID__ || "__unknown__";
}

function __fluxEncodeUtf8(input) {
    const encoded = unescape(encodeURIComponent(String(input)));
    const bytes = new Uint8Array(encoded.length);
    for (let i = 0; i < encoded.length; i++) {
        bytes[i] = encoded.charCodeAt(i);
    }
    return bytes;
}

function __fluxDecodeUtf8(input) {
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input ?? []);
    let binary = "";
    const chunkSize = 0x8000;
    for (let i = 0; i < bytes.length; i += chunkSize) {
        const chunk = bytes.subarray(i, i + chunkSize);
        binary += String.fromCharCode(...chunk);
    }
    return decodeURIComponent(escape(binary));
}

class TextEncoder {
    encode(input = "") {
        return __fluxEncodeUtf8(input);
    }
}

class TextDecoder {
    decode(input = undefined) {
        if (input == null) return "";
        return __fluxDecodeUtf8(input);
    }
}

globalThis.TextEncoder = globalThis.TextEncoder || TextEncoder;
globalThis.TextDecoder = globalThis.TextDecoder || TextDecoder;

const __fluxEncoder = new globalThis.TextEncoder();
const __fluxDecoder = new globalThis.TextDecoder();

function __fluxNormalizeHeaderName(name) {
    return String(name).toLowerCase();
}

function __fluxBodyToText(body) {
    if (body == null) return null;
    if (typeof body === "string") return body;
    if (body instanceof Uint8Array) return __fluxDecoder.decode(body);
    return String(body);
}

function __fluxCreateBodyState(bodyText) {
    return {
        bodyText,
        used: false,
        locked: false,
        emitted: false,
    };
}

function __fluxConsumeBodyText(state) {
    if (state.used) {
        throw new TypeError("Body already consumed");
    }
    state.used = true;
    state.emitted = true;
    return state.bodyText ?? "";
}

function __fluxCreateBodyStream(state) {
    return {
        getReader() {
            if (state.locked) {
                throw new TypeError("ReadableStream is locked");
            }
            state.locked = true;
            return {
                async read() {
                    if (state.emitted || state.bodyText === null) {
                        state.used = true;
                        state.emitted = true;
                        return { value: undefined, done: true };
                    }
                    state.used = true;
                    state.emitted = true;
                    return {
                        value: __fluxEncoder.encode(state.bodyText),
                        done: false,
                    };
                },
                releaseLock() {
                    state.locked = false;
                },
                async cancel() {
                    state.used = true;
                    state.emitted = true;
                    state.locked = false;
                },
            };
        },
    };
}

function __fluxDecodeFormComponent(value) {
    return decodeURIComponent(String(value).replace(/\+/g, " "));
}

class URLSearchParams {
    constructor(init = "") {
        this._pairs = [];

        if (init instanceof URLSearchParams) {
            this._pairs = [...init._pairs];
            return;
        }

        if (Array.isArray(init)) {
            for (const [key, value] of init) {
                this.append(key, value);
            }
            return;
        }

        if (typeof init === "string") {
            const query = init.startsWith("?") ? init.slice(1) : init;
            if (!query) return;
            for (const pair of query.split("&")) {
                if (!pair) continue;
                const [rawKey, rawValue = ""] = pair.split("=");
                this.append(__fluxDecodeFormComponent(rawKey), __fluxDecodeFormComponent(rawValue));
            }
            return;
        }

        if (init && typeof init === "object") {
            for (const [key, value] of Object.entries(init)) {
                this.append(key, value);
            }
        }
    }

    append(name, value) {
        this._pairs.push([String(name), String(value)]);
    }

    get(name) {
        const key = String(name);
        const match = this._pairs.find(([candidate]) => candidate === key);
        return match ? match[1] : null;
    }

    getAll(name) {
        const key = String(name);
        return this._pairs
            .filter(([candidate]) => candidate === key)
            .map(([, value]) => value);
    }

    has(name) {
        const key = String(name);
        return this._pairs.some(([candidate]) => candidate === key);
    }

    set(name, value) {
        const key = String(name);
        const nextValue = String(value);
        const nextPairs = [];
        let replaced = false;

        for (const [candidate, currentValue] of this._pairs) {
            if (candidate === key) {
                if (!replaced) {
                    nextPairs.push([key, nextValue]);
                    replaced = true;
                }
                continue;
            }
            nextPairs.push([candidate, currentValue]);
        }

        if (!replaced) {
            nextPairs.push([key, nextValue]);
        }

        this._pairs = nextPairs;
    }

    delete(name) {
        const key = String(name);
        this._pairs = this._pairs.filter(([candidate]) => candidate !== key);
    }

    entries() {
        return this._pairs[Symbol.iterator]();
    }

    [Symbol.iterator]() {
        return this.entries();
    }

    toString() {
        return this._pairs
            .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`)
            .join("&");
    }
}

class URL {
    constructor(input, base = undefined) {
        const parsed = Deno.core.ops.op_flux_parse_url(String(input), base == null ? "" : String(base));
        this.href = parsed.href;
        this.origin = parsed.origin;
        this.protocol = parsed.protocol;
        this.username = parsed.username;
        this.password = parsed.password ?? "";
        this.host = parsed.host;
        this.hostname = parsed.hostname;
        this.port = parsed.port;
        this.pathname = parsed.pathname;
        this.search = parsed.search;
        this.hash = parsed.hash;
        this.searchParams = new URLSearchParams(this.search);
    }

    toString() {
        return this.href;
    }

    toJSON() {
        return this.href;
    }
}

class Headers {
    constructor(init = undefined) {
        this._map = new Map();
        if (init instanceof Headers) {
            for (const [key, value] of init.entries()) this.append(key, value);
            return;
        }
        if (Array.isArray(init)) {
            for (const [key, value] of init) this.append(key, value);
            return;
        }
        if (init && typeof init === "object") {
            for (const [key, value] of Object.entries(init)) this.append(key, value);
        }
    }

    append(name, value) {
        const key = __fluxNormalizeHeaderName(name);
        const existing = this._map.get(key);
        const next = String(value);
        this._map.set(key, existing ? `${existing}, ${next}` : next);
    }

    delete(name) {
        this._map.delete(__fluxNormalizeHeaderName(name));
    }

    get(name) {
        const value = this._map.get(__fluxNormalizeHeaderName(name));
        return value === undefined ? null : value;
    }

    has(name) {
        return this._map.has(__fluxNormalizeHeaderName(name));
    }

    set(name, value) {
        this._map.set(__fluxNormalizeHeaderName(name), String(value));
    }

    entries() {
        return this._map.entries();
    }

    keys() {
        return this._map.keys();
    }

    values() {
        return this._map.values();
    }

    forEach(callback, thisArg = undefined) {
        for (const [key, value] of this._map.entries()) {
            callback.call(thisArg, value, key, this);
        }
    }

    [Symbol.iterator]() {
        return this.entries();
    }
}

class Request {
    constructor(input, init = undefined) {
        const source = input instanceof Request ? input : null;
        const options = init || {};

        this.url = source ? source.url : String(input);
        this.method = String(options.method || (source ? source.method : "GET")).toUpperCase();
        this.headers = new Headers(options.headers || (source ? source.headers : undefined));
        this._bodyState = __fluxCreateBodyState(options.body !== undefined
            ? __fluxBodyToText(options.body)
            : source
                ? source._bodyState.bodyText
                : null);
        this._bodyStream = null;
    }

    get bodyUsed() {
        return this._bodyState.used;
    }

    get body() {
        if (this._bodyState.bodyText === null) return null;
        if (!this._bodyStream) {
            this._bodyStream = __fluxCreateBodyStream(this._bodyState);
        }
        return this._bodyStream;
    }

    async text() {
        return __fluxConsumeBodyText(this._bodyState);
    }

    async json() {
        return JSON.parse(await this.text());
    }
}

class Response {
    constructor(body = null, init = undefined) {
        const options = init || {};
        this._bodyState = __fluxCreateBodyState(__fluxBodyToText(body));
        this._bodyStream = null;
        this.status = options.status ?? 200;
        this.statusText = options.statusText ?? "";
        this.headers = new Headers(options.headers);
    }

    static json(value, init = undefined) {
        const options = init || {};
        const headers = new Headers(options.headers);
        if (!headers.has("content-type")) {
            headers.set("content-type", "application/json");
        }
        return new Response(JSON.stringify(value), {
            ...options,
            headers,
        });
    }

    get ok() {
        return this.status >= 200 && this.status < 300;
    }

    get bodyUsed() {
        return this._bodyState.used;
    }

    get body() {
        if (this._bodyState.bodyText === null) return null;
        if (!this._bodyStream) {
            this._bodyStream = __fluxCreateBodyStream(this._bodyState);
        }
        return this._bodyStream;
    }

    async text() {
        return __fluxConsumeBodyText(this._bodyState);
    }

    async json() {
        return JSON.parse(await this.text());
    }

    clone() {
        if (this.bodyUsed) {
            throw new TypeError("Body already consumed");
        }
        return new Response(this._bodyState.bodyText, {
            status: this.status,
            statusText: this.statusText,
            headers: this.headers,
        });
    }
}

globalThis.URLSearchParams = globalThis.URLSearchParams || URLSearchParams;
globalThis.URL = globalThis.URL || URL;
globalThis.Headers = Headers;
globalThis.Request = Request;
globalThis.Response = Response;

// ── crypto ──────────────────────────────────────────────────────────────────
if (!globalThis.crypto) globalThis.crypto = {};
globalThis.crypto.getRandomValues = (typedArray) => {
    if (!typedArray || typeof typedArray.length !== "number") {
        throw new TypeError("crypto.getRandomValues expected a typed array");
    }
    for (let i = 0; i < typedArray.length; i++) {
        typedArray[i] = Math.floor(Deno.core.ops.op_random(__flux_eid()) * 256);
    }
    return typedArray;
};
globalThis.crypto.randomUUID = () => Deno.core.ops.op_random_uuid(__flux_eid());

// ── fetch ──────────────────────────────────────────────────────────────────
globalThis.fetch = async function(input, init = undefined) {
    const request = input instanceof Request ? input : new Request(input, init);
    const method = request.method || "GET";
    const headers = Object.fromEntries(request.headers.entries());
    const body = (method === "GET" || method === "HEAD") ? null : await request.text();
    const response = await Deno.core.ops.op_flux_fetch({
        execution_id: __flux_eid(),
        url: String(request.url),
        method: String(method),
        body,
        headers,
    });

    const responseBody = response.body == null
        ? null
        : typeof response.body === "string"
            ? response.body
            : JSON.stringify(response.body);

    return new Response(responseBody, {
        status: response.status,
        headers: new Headers(response.headers ?? {}),
    });
};

// ── Date.now() + new Date() ────────────────────────────────────────────────
{
  const _OrigDate = globalThis.Date;
  class PatchedDate extends _OrigDate {
    constructor(...args) {
      if (args.length === 0) {
        super(Deno.core.ops.op_flux_now(__flux_eid()));
      } else {
        super(...args);
      }
    }
  }
    PatchedDate.now = function() { return Deno.core.ops.op_flux_now(__flux_eid()); };
  globalThis.Date = PatchedDate;
}

// ── performance.now() ──────────────────────────────────────────────────────
if (globalThis.performance) {
    globalThis.performance.now = function() { return Deno.core.ops.op_flux_now(__flux_eid()); };
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

    const requestHeaders = new Headers(headersInit);
    let requestUrl = String(url);
    const hasScheme = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(requestUrl);
    if (!hasScheme) {
        const host = requestHeaders.get("host") || "127.0.0.1";
        if (requestUrl.startsWith("/")) {
            requestUrl = `http://${host}${requestUrl}`;
        } else {
            requestUrl = `http://${requestUrl}`;
        }
    }

    const request = new Request(requestUrl, {
    method,
        headers: requestHeaders,
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
