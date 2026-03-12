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

use std::collections::HashMap;
use wasmtime::{
    Caller, Config, Engine, Linker, Module, Store,
};
use tokio::time::{timeout, Duration};

use crate::engine::executor::{ExecutionResult, LogLine};

// ─── HostState ─────────────────────────────────────────────────────────────

/// Data owned by the Wasmtime `Store` — accessible from host import callbacks.
pub struct HostState {
    pub secrets: HashMap<String, String>,
    pub logs:    Vec<LogLine>,
    /// Scratch buffer used by `secrets_get` to write values back to WASM memory.
    pub secrets_scratch: Vec<u8>,
}

// ─── Params ────────────────────────────────────────────────────────────────

pub struct WasmExecutionParams {
    /// Raw WASM bytecode.
    pub bytes:        Vec<u8>,
    pub secrets:      HashMap<String, String>,
    pub payload:      serde_json::Value,
    pub tenant_id:    String,
    /// Per-function key used to look up the compiled `Module` in the pool cache.
    pub function_id:  String,
    /// Maximum WASM CPU fuel (instructions).  1 billion ≈ a few hundred ms.
    pub fuel_limit:   u64,
}

impl Default for WasmExecutionParams {
    fn default() -> Self {
        Self {
            bytes:       Vec::new(),
            secrets:     HashMap::new(),
            payload:     serde_json::Value::Null,
            tenant_id:   String::new(),
            function_id: String::new(),
            fuel_limit:  1_000_000_000,
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
        secrets:         params.secrets,
        logs:            Vec::new(),
        secrets_scratch: vec![0u8; 4096],
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

// ─── WASM binary validation ────────────────────────────────────────────────

/// Validate that a WASM binary exports the required symbols.
/// Called by `flux deploy` before uploading a WASM bundle.
pub fn validate_wasm_exports(bytes: &[u8]) -> Result<(), String> {
    // Check magic bytes: 0x00 0x61 0x73 0x6D
    if bytes.len() < 4 || &bytes[0..4] != b"\x00asm" {
        return Err("not a valid WASM binary (bad magic bytes)".to_string());
    }

    let engine = build_engine();
    let module = compile_module(&engine, bytes)?;

    let exports: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
    let required = ["handle", "__flux_alloc", "memory"];

    let missing: Vec<&str> = required
        .iter()
        .filter(|req| !exports.iter().any(|e| e.as_str() == **req))
        .copied()
        .collect();

    if !missing.is_empty() {
        return Err(format!(
            "WASM module is missing required exports: {}. \
             All three are required: handle, __flux_alloc, memory. \
             Use the flux-wasm-sdk crate — the flux_handler! macro generates them automatically.",
            missing.join(", ")
        ));
    }

    Ok(())
}
