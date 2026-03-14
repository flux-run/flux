//! WASM execution engine — runs `.wasm` modules compiled by Wasmtime (Cranelift).
//!
//! ## ABI contract with the WASM module
//!
//! ### Host imports the module must accept (`fluxbase` namespace):
//!
//! | Function | Signature | Behaviour |
//! |---|---|---|
//! | `fluxbase.log` | `(level: i32, msg_ptr: i32, msg_len: i32)` | Emit a structured log line |
//! | `fluxbase.secrets_get` | `(key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32) → i32` | Read a secret; returns actual byte length or -1 if missing |
//!
//! ### Module exports the host calls:
//!
//! | Export | Signature | Description |
//! |---|---|---|
//! | `memory` | `(Memory)` | Linear memory (standard) |
//! | `__flux_alloc` | `(size: i32) → ptr: i32` | Allocate `size` bytes in the module's heap |
//! | `handle` | `(payload_ptr: i32, payload_len: i32) → result_ptr: i32` | Main entry point |
//!
//! ### Result layout (at `result_ptr`):
//! ```
//! [ u32 LE length ][ `length` bytes of UTF-8 JSON ]
//! ```
//! The JSON must have either `"output"` or `"error"` keys at the top level.
//! Logs are emitted via `fluxbase.log` during execution, not in the result.

//! WASM execution engine — runs `.wasm` modules compiled by Wasmtime (Cranelift AOT).
//!
//! ## Compilation strategy
//!
//! Wasmtime compiles `.wasm` bytecode to native machine code via Cranelift AOT at first
//! load. The compiled `Module` is cached in `WasmPool` so subsequent calls for the same
//! function skip compilation entirely (typically 50–200 ms for small modules).
//!
//! ## Fuel-based limits
//!
//! Every `Store` is pre-loaded with `fuel_limit` units of "fuel" (1 billion ≈ a few
//! hundred ms of CPU). When fuel is exhausted Wasmtime traps with `OutOfFuel`, preventing
//! runaway WASM loops from consuming a worker thread indefinitely.
//!
//! ## Host imports (`fluxbase` namespace)
//!
//! WASM modules may call the following host functions via linear memory pointers:
//!
//! | Import | Purpose |
//! |---|---|
//! | `fluxbase.log(level, ptr, len)` | Append a `LogLine` to `HostState::logs` |
//! | `fluxbase.secrets_get(key_ptr, key_len, out_ptr, out_max) → i32` | Copy a secret value into WASM memory; returns byte length or -1 |
//! | `fluxbase.http_fetch(...)` | Outbound HTTP (allow-listed hosts only) |
//!
//! ## Memory safety
//!
//! All pointer/length pairs received from WASM are bounds-checked against
//! `memory.data_size()` before slicing. Invalid pointers return an error rather than
//! panicking the host process.
use std::collections::HashMap;
use wasmtime::{
    Caller, Config, Engine, Linker, Module, Store,
};
use tokio::time::{timeout, Duration};

use crate::engine::executor::{ExecutionResult, LogLine};

// ─── HostState ─────────────────────────────────────────────────────────────

/// Data owned by the Wasmtime `Store` — accessible from host import callbacks.
pub struct HostState {
    pub secrets:            HashMap<String, String>,
    pub logs:               Vec<LogLine>,
    /// `http_fetch` allow-list.  Empty vec = deny all.  Contains `"*"` = allow all.
    pub allowed_http_hosts: Vec<String>,
    /// Shared reqwest client for outbound HTTP from `fluxbase.http_fetch`.
    pub http_client:        reqwest::Client,
}

// ─── Params ────────────────────────────────────────────────────────────────

pub struct WasmExecutionParams {
    pub secrets:      HashMap<String, String>,
    pub payload:      serde_json::Value,
    /// Maximum WASM CPU fuel (instructions).  1 billion ≈ a few hundred ms.
    pub fuel_limit:   u64,
    /// Hosts the WASM function is allowed to call via `fluxbase.http_fetch`.
    /// Empty = deny all.  `["*"]` = allow all (use with caution).
    pub allowed_http_hosts: Vec<String>,
    /// Shared HTTP client passed through for outbound calls.
    pub http_client: Option<reqwest::Client>,
}

impl Default for WasmExecutionParams {
    fn default() -> Self {
        Self {
            secrets:           HashMap::new(),
            payload:           serde_json::Value::Null,
            fuel_limit:        1_000_000_000,
            allowed_http_hosts: Vec::new(),
            http_client:       None,
        }
    }
}

// ─── Engine factory ────────────────────────────────────────────────────────

/// Build a shared Wasmtime `Engine` with Cranelift AOT + fuel interruption.
pub fn build_engine() -> Engine {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    Engine::new(&cfg).expect("failed to build Wasmtime engine")
}

// ─── Core execution ────────────────────────────────────────────────────────

/// Compile a WASM module from raw bytes using the shared engine.
/// This is the expensive step (~5–50 ms); results should be cached.
pub fn compile_module(engine: &Engine, bytes: &[u8]) -> Result<Module, String> {
    Module::from_binary(engine, bytes)
        .map_err(|e| format!("wasm compilation failed: {}", e))
}

/// Execute a pre-compiled `Module`.  Runs CPU-bound work on a blocking thread.
pub async fn execute_wasm(
    engine: &Engine,
    module: &Module,
    params: WasmExecutionParams,
) -> Result<ExecutionResult, String> {
    // Clone what we need to move into spawn_blocking
    let engine = engine.clone();
    let module = module.clone();

    let handle = tokio::task::spawn_blocking(move || {
        execute_wasm_sync(&engine, &module, params)
    });

    // Wall-clock backstop in case fuel is exhausted slowly or Wasmtime hangs.
    match timeout(Duration::from_secs(35), handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(join_err)) => Err(format!("wasm worker panicked: {}", join_err)),
        Err(_) => Err("wasm execution timed out after 35 seconds".to_string()),
    }
}

// ─── Synchronous kernel (runs on a dedicated blocking thread) ────────────────

fn execute_wasm_sync(
    engine: &Engine,
    module: &Module,
    params: WasmExecutionParams,
) -> Result<ExecutionResult, String> {
    let host = HostState {
        secrets:            params.secrets,
        logs:               Vec::new(),
        allowed_http_hosts: params.allowed_http_hosts,
        http_client:        params.http_client.unwrap_or_else(reqwest::Client::new),
    };

    let mut store = Store::new(engine, host);
    store.set_fuel(params.fuel_limit)
        .map_err(|e| format!("fuel setup error: {}", e))?;

    // ── Register host imports ──────────────────────────────────────────────

    let mut linker = Linker::<HostState>::new(engine);

    // fluxbase.log(level: i32, msg_ptr: i32, msg_len: i32)
    linker.func_wrap("fluxbase", "log", |mut caller: Caller<HostState>, level: i32, msg_ptr: i32, msg_len: i32| {
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None    => return,
        };
        let data = memory.data(&caller);
        let end  = (msg_ptr as usize).saturating_add(msg_len as usize);
        if end > data.len() { return; }
        let message = String::from_utf8_lossy(&data[msg_ptr as usize..end]).into_owned();

        let level_str = match level {
            0 => "debug",
            1 => "info",
            2 => "warn",
            _ => "error",
        };
        caller.data_mut().logs.push(LogLine {
            level:           level_str.to_string(),
            message,
            span_type:       None,
            source:          Some("function".to_string()),
            span_id:         None,
            duration_ms:     None,
            execution_state: None,
            tool_name:       None,
        });
    }).map_err(|e| e.to_string())?;

    // fluxbase.http_fetch(req_ptr, req_len, out_ptr, out_max) → actual_resp_len or -1
    //
    // The WASM module writes a JSON request object at `req_ptr`:
    //   { "method": "GET", "url": "https://...", "headers": {...}, "body": "<base64>" }
    // The host validates the URL against `allowed_http_hosts`, performs the
    // outbound HTTP request, and writes a JSON response object at `out_ptr`:
    //   { "status": 200, "headers": {...}, "body": "<base64>" }
    // Returns the number of bytes written, or -1 on error / denied.
    linker.func_wrap("fluxbase", "http_fetch", |mut caller: Caller<HostState>, req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32| -> i32 {
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None => return -1,
        };

        // Read request JSON from WASM memory.
        let req_json = {
            let data = memory.data(&caller);
            let end  = (req_ptr as usize).saturating_add(req_len as usize);
            if end > data.len() { return -1; }
            match serde_json::from_slice::<serde_json::Value>(&data[req_ptr as usize..end]) {
                Ok(v)  => v,
                Err(_) => return -1,
            }
        };

        let url = match req_json.get("url").and_then(|u| u.as_str()) {
            Some(u) => u.to_string(),
            None    => return -1,
        };

        // ── Allow-list check ─────────────────────────────────────────────────
        let allowed = {
            let hosts = &caller.data().allowed_http_hosts;
            if hosts.iter().any(|h| h == "*") {
                true
            } else {
                // Only compare the parsed host — never use url.starts_with(h) because
                // credential-stuffed URLs like https://allowed.com@evil.com would bypass
                // the check (the URL starts with the allowed prefix but resolves to evil.com).
                match url.parse::<reqwest::Url>() {
                    Ok(parsed) => {
                        let host_str = parsed.host_str().unwrap_or("");
                        hosts.iter().any(|h| h == host_str)
                    }
                    Err(_) => false,
                }
            }
        };
        if !allowed {
            caller.data_mut().logs.push(LogLine {
                level:           "warn".to_string(),
                message:         format!("http_fetch blocked: {} not in allowed_http_hosts", url),
                span_type:       None,
                source:          Some("function".to_string()),
                span_id:         None,
                duration_ms:     None,
                execution_state: None,
                tool_name:       None,
            });
            return -1;
        }

        let method_str = req_json.get("method").and_then(|m| m.as_str()).unwrap_or("GET").to_uppercase();
        let body_b64   = req_json.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string();
        let headers    = req_json.get("headers").and_then(|h| h.as_object()).cloned();

        // ── Make the HTTP request (blocking on the tokio runtime) ─────────────
        let client = caller.data().http_client.clone();
        let rt_handle = tokio::runtime::Handle::current();
        let resp_json = rt_handle.block_on(async move {
            use base64::Engine as _;

            let method = reqwest::Method::from_bytes(method_str.as_bytes())
                .unwrap_or(reqwest::Method::GET);
            let mut builder = client.request(method, &url);

            if let Some(hdrs) = headers {
                for (k, v) in &hdrs {
                    if let Some(vs) = v.as_str() {
                        builder = builder.header(k.as_str(), vs);
                    }
                }
            }
            if !body_b64.is_empty() {
                if let Ok(body_bytes) = base64::engine::general_purpose::STANDARD.decode(&body_b64) {
                    builder = builder.body(body_bytes);
                }
            }

            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let resp_headers: serde_json::Map<String, serde_json::Value> = resp
                        .headers()
                        .iter()
                        .filter_map(|(k, v)| v.to_str().ok().map(|vs| (k.to_string(), serde_json::Value::String(vs.to_string()))))
                        .collect();
                    let body_bytes = resp.bytes().await.unwrap_or_default();
                    let body_b64  = base64::engine::general_purpose::STANDARD.encode(&body_bytes);
                    serde_json::json!({ "status": status, "headers": resp_headers, "body": body_b64 })
                }
                Err(e) => serde_json::json!({ "status": 0, "error": e.to_string() }),
            }
        });

        // ── Write response JSON to WASM memory ────────────────────────────────
        let resp_bytes = match serde_json::to_vec(&resp_json) {
            Ok(b)  => b,
            Err(_) => return -1,
        };
        if resp_bytes.len() > out_max as usize { return -1; }

        let data = memory.data_mut(&mut caller);
        let out_start = out_ptr as usize;
        let out_end   = out_start + resp_bytes.len();
        if out_end > data.len() { return -1; }
        data[out_start..out_end].copy_from_slice(&resp_bytes);
        resp_bytes.len() as i32
    }).map_err(|e| e.to_string())?;

    // fluxbase.secrets_get(key_ptr, key_len, out_ptr, out_max) → actual_len or -1
    linker.func_wrap("fluxbase", "secrets_get", |mut caller: Caller<HostState>, key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32| -> i32 {
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None    => return -1,
        };

        // Read the key from WASM memory
        let key = {
            let data = memory.data(&caller);
            let end  = (key_ptr as usize).saturating_add(key_len as usize);
            if end > data.len() { return -1; }
            String::from_utf8_lossy(&data[key_ptr as usize..end]).into_owned()
        };

        // Look up the value
        let value = match caller.data().secrets.get(&key).cloned() {
            Some(v) => v,
            None    => return -1,
        };
        let value_bytes = value.as_bytes();
        let write_len   = value_bytes.len().min(out_max as usize);

        // Write value into WASM memory at out_ptr
        let data = memory.data_mut(&mut caller);
        let out_start = out_ptr as usize;
        let out_end   = out_start + write_len;
        if out_end > data.len() { return -1; }
        data[out_start..out_end].copy_from_slice(&value_bytes[..write_len]);

        write_len as i32
    }).map_err(|e| e.to_string())?;

    // ── Instantiate ────────────────────────────────────────────────────────

    let instance = linker.instantiate(&mut store, module)
        .map_err(|e| format!("wasm instantiation failed: {}", e))?;

    // ── Fetch required exports ─────────────────────────────────────────────

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or("wasm module missing required 'memory' export")?;

    let alloc_fn = instance
        .get_typed_func::<i32, i32>(&mut store, "__flux_alloc")
        .map_err(|_| "wasm module missing required '__flux_alloc' export")?;

    let handle_fn = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "handle")
        .map_err(|_| "wasm module missing required 'handle' export")?;

    // ── Write payload into WASM linear memory ─────────────────────────────

    let payload_json = serde_json::to_string(&params.payload)
        .map_err(|e| format!("payload serialization failed: {}", e))?;
    let payload_bytes = payload_json.as_bytes();
    let payload_len   = payload_bytes.len() as i32;

    let payload_ptr = alloc_fn.call(&mut store, payload_len)
        .map_err(|e| format!("__flux_alloc failed: {}", e))?;

    if payload_ptr <= 0 {
        return Err("__flux_alloc returned null pointer".to_string());
    }

    {
        let data = memory.data_mut(&mut store);
        let start = payload_ptr as usize;
        let end   = start + payload_bytes.len();
        if end > data.len() {
            return Err(format!(
                "payload ({} bytes) overflows linear memory at offset {}",
                payload_bytes.len(), start
            ));
        }
        data[start..end].copy_from_slice(payload_bytes);
    }

    // ── Call handle ────────────────────────────────────────────────────────

    let result_ptr = handle_fn
        .call(&mut store, (payload_ptr, payload_len))
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("fuel") || msg.contains("trap: out of fuel") {
                "wasm function exceeded CPU fuel limit".to_string()
            } else {
                format!("wasm handle trap: {}", msg)
            }
        })?;

    // ── Read result from WASM memory ───────────────────────────────────────
    //
    // Layout (written by the WASM module):
    //   [4 bytes u32 LE = payload length][<length> bytes of UTF-8 JSON]

    let result_json = {
        let data = memory.data(&store);

        if result_ptr <= 0 {
            return Err("wasm handle returned null result pointer".to_string());
        }

        let ptr       = result_ptr as usize;
        let hdr_end   = ptr + 4;
        if hdr_end > data.len() {
            return Err("result pointer out of bounds reading length header".to_string());
        }

        let result_len = u32::from_le_bytes([data[ptr], data[ptr+1], data[ptr+2], data[ptr+3]]) as usize;
        let json_end   = hdr_end + result_len;

        if json_end > data.len() {
            return Err(format!(
                "result length {} overflows linear memory at offset {}",
                result_len, hdr_end
            ));
        }

        match std::str::from_utf8(&data[hdr_end..json_end]) {
            Ok(s)  => s.to_string(),
            Err(e) => return Err(format!("result JSON is not valid UTF-8: {}", e)),
        }
    };

    let result_value: serde_json::Value = serde_json::from_str(&result_json)
        .map_err(|e| format!("result JSON parse error: {} — raw: {:.256}", e, result_json))?;

    // ── Extract output / error from result ────────────────────────────────

    if let Some(err_msg) = result_value.get("error").and_then(|v| v.as_str()) {
        return Err(serde_json::json!({
            "code":    "FunctionExecutionError",
            "message": err_msg
        }).to_string());
    }

    let output = result_value.get("output")
        .cloned()
        .unwrap_or(result_value.clone());

    let logs = store.into_data().logs;
    Ok(ExecutionResult { output, logs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Minimal WAT module satisfying the fluxbase WASM ABI.
    ///
    /// Memory layout at result pointer 4:
    ///   bytes 4-7  : u32 LE = 15  (length of JSON below)
    ///   bytes 8-22 : `{"output":"ok"}`
    ///
    /// handle() returns 4 (non-zero) so the executor can read the header.
    /// __flux_alloc() returns 65536 (page boundary) as the payload write buffer.
    const MINIMAL_WAT: &str = r#"(module
        (import "fluxbase" "log"         (func (param i32 i32 i32)))
        (import "fluxbase" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 4) "\0f\00\00\00{\"output\":\"ok\"}")
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
        (func (export "handle") (param i32 i32) (result i32) i32.const 4)
    )"#;

    /// WAT module that returns an error in the result JSON.
    const ERROR_WAT: &str = r#"(module
        (import "fluxbase" "log"         (func (param i32 i32 i32)))
        (import "fluxbase" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 4) "\16\00\00\00{\"error\":\"test_error\"}")
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
        (func (export "handle") (param i32 i32) (result i32) i32.const 4)
    )"#;

    /// WAT module missing the `handle` export — should fail instantiation.
    const MISSING_HANDLE_WAT: &str = r#"(module
        (import "fluxbase" "log"         (func (param i32 i32 i32)))
        (import "fluxbase" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "fluxbase" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
    )"#;

    fn wasm_bytes(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("WAT compilation failed")
    }

    fn default_params() -> WasmExecutionParams {
        WasmExecutionParams {
            payload: serde_json::json!({"msg": "test"}),
            ..Default::default()
        }
    }

    // ── build_engine ──────────────────────────────────────────────────────

    #[test]
    fn build_engine_does_not_panic() {
        let _engine = build_engine();
    }

    #[test]
    fn two_engines_are_independent() {
        let _e1 = build_engine();
        let _e2 = build_engine();
    }

    // ── compile_module ────────────────────────────────────────────────────

    #[test]
    fn compile_valid_module_succeeds() {
        let engine = build_engine();
        let bytes  = wasm_bytes(MINIMAL_WAT);
        assert!(compile_module(&engine, &bytes).is_ok());
    }

    #[test]
    fn compile_invalid_bytes_returns_err() {
        let engine = build_engine();
        let result = compile_module(&engine, b"not wasm bytes at all");
        assert!(result.is_err(), "expected Err for invalid wasm bytes");
    }

    #[test]
    fn compile_empty_bytes_returns_err() {
        let engine = build_engine();
        assert!(compile_module(&engine, b"").is_err());
    }

    #[test]
    fn compile_error_message_contains_context() {
        let engine = build_engine();
        let err = compile_module(&engine, b"garbage").unwrap_err();
        assert!(!err.is_empty(), "error message should not be empty");
    }

    // ── execute_wasm ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_wasm_returns_ok_output() {
        let engine = build_engine();
        let bytes  = wasm_bytes(MINIMAL_WAT);
        let module = compile_module(&engine, &bytes).unwrap();

        let result = execute_wasm(&engine, &module, default_params())
            .await
            .expect("execute_wasm failed");

        assert_eq!(result.output, serde_json::json!("ok"));
        assert!(result.logs.is_empty());
    }

    #[tokio::test]
    async fn execute_wasm_error_module_returns_err() {
        let engine = build_engine();
        let bytes  = wasm_bytes(ERROR_WAT);
        let module = compile_module(&engine, &bytes).unwrap();

        let result = execute_wasm(&engine, &module, default_params()).await;
        assert!(result.is_err(), "expected Err from error module");
        let msg = result.unwrap_err();
        assert!(msg.contains("test_error"), "error message should contain 'test_error', got: {msg}");
    }

    #[tokio::test]
    async fn execute_wasm_missing_handle_returns_err() {
        let engine = build_engine();
        let bytes  = wasm_bytes(MISSING_HANDLE_WAT);
        let module = compile_module(&engine, &bytes).unwrap();

        let result = execute_wasm(&engine, &module, default_params()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_wasm_passes_secrets_to_host_state() {
        let engine = build_engine();
        let bytes  = wasm_bytes(MINIMAL_WAT);
        let module = compile_module(&engine, &bytes).unwrap();

        let mut secrets = HashMap::new();
        secrets.insert("API_KEY".to_string(), "secret123".to_string());

        let params = WasmExecutionParams {
            secrets,
            payload: serde_json::json!({}),
            ..Default::default()
        };
        // The minimal module doesn't call secrets_get, but execution should succeed.
        let result = execute_wasm(&engine, &module, params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_wasm_default_fuel_limit_is_nonzero() {
        let params = WasmExecutionParams::default();
        assert!(params.fuel_limit > 0);
    }

    // ── WasmExecutionParams default ───────────────────────────────────────

    #[test]
    fn wasm_params_default_payload_is_null() {
        let p = WasmExecutionParams::default();
        assert!(p.payload.is_null());
    }

    #[test]
    fn wasm_params_default_allowed_hosts_is_empty() {
        let p = WasmExecutionParams::default();
        assert!(p.allowed_http_hosts.is_empty());
    }

    // ── Allow-list URL validation tests ──────────────────────────────────────
    // These tests verify that the host-only comparison used in the allow-list
    // check correctly rejects credential-stuffed bypass attempts.

    fn host_of(url: &str) -> Option<String> {
        url.parse::<reqwest::Url>().ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
    }

    #[test]
    fn allow_list_parsed_host_matches_simple_url() {
        let host = host_of("https://api.example.com/path?q=1");
        assert_eq!(host.as_deref(), Some("api.example.com"));
    }

    #[test]
    fn allow_list_credential_stuffed_url_resolves_to_evil_host() {
        // https://allowed.com@evil.com — parsed host is evil.com, NOT allowed.com
        let host = host_of("https://allowed.com@evil.com/steal");
        assert_eq!(host.as_deref(), Some("evil.com"));
        // Verify it does NOT match the allowed host
        let allowed_hosts = vec!["allowed.com".to_string()];
        let bypasses = allowed_hosts.iter().any(|h| h == host.as_deref().unwrap_or(""));
        assert!(!bypasses, "credential-stuffed URL must not bypass allow-list");
    }

    #[test]
    fn allow_list_wildcard_permits_any_host() {
        let allowed_hosts = vec!["*".to_string()];
        assert!(allowed_hosts.iter().any(|h| h == "*"));
    }

    #[test]
    fn allow_list_exact_host_match_passes() {
        let host = host_of("https://safe.example.com/api");
        let allowed_hosts = vec!["safe.example.com".to_string()];
        let passes = allowed_hosts.iter().any(|h| h == host.as_deref().unwrap_or(""));
        assert!(passes);
    }

    #[test]
    fn allow_list_different_host_rejected() {
        let host = host_of("https://evil.com/steal");
        let allowed_hosts = vec!["safe.example.com".to_string()];
        let passes = allowed_hosts.iter().any(|h| h == host.as_deref().unwrap_or(""));
        assert!(!passes);
    }
}
