use serde::{Deserialize, Serialize};

// ── Fluxbase host imports ─────────────────────────────────────────────────────
//
// The Flux runtime exposes these functions to every WASM module.
// All I/O is async at the OS level: the WASM fiber suspends while the runtime
// handles the call, so no OS threads are blocked.
//
// Memory convention (Flux WASM ABI):
//   1. Host calls __flux_alloc(payload_len) -> ptr and writes the JSON payload.
//   2. Host calls handle(ptr, len) -> result_ptr.
//   3. Result at result_ptr: [ u32 LE byte-length ][ <length> bytes of UTF-8 JSON ].
//      JSON must be {"output": ...} on success or {"error": "..."} on failure.
#[link(wasm_import_module = "fluxbase")]
extern "C" {
    /// Emit a structured log line.
    /// level: 0=debug, 1=info, 2=warn, 3=error
    fn log(level: i32, msg_ptr: i32, msg_len: i32);

    /// Execute a raw SQL query via the data-engine.
    /// sql_ptr/sql_len: UTF-8 SQL string.
    /// params_ptr/params_len: JSON array of positional parameters (e.g. `[42,"foo"]`),
    ///   or 0/0 for no parameters.
    /// out_ptr/out_max: output buffer; host writes JSON rows array.
    /// Returns actual bytes written, or negative on error.
    fn db_query(
        sql_ptr: i32, sql_len: i32,
        params_ptr: i32, params_len: i32,
        out_ptr: i32, out_max: i32,
    ) -> i32;

    /// Enqueue an async job.
    /// req_ptr/req_len: JSON object `{ "function": "name", "payload": {...}, "delay": "5m" }`.
    /// out_ptr/out_max: output buffer; host writes `{ "job_id": "..." }`.
    /// Returns actual bytes written, or negative on error.
    fn queue_push(req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32) -> i32;

    /// SSRF-protected HTTP fetch.
    /// req_ptr/req_len: JSON object `{ "url": "...", "method": "POST", "headers": {...}, "body": "..." }`.
    /// out_ptr/out_max: output buffer; host writes `{ "status": 200, "ok": true, "body": "..." }`.
    /// Returns actual bytes written, or negative on error.
    fn http_fetch(req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32) -> i32;

    /// Read a secret value.
    /// key_ptr/key_len: UTF-8 secret name.
    /// out_ptr/out_max: output buffer; host writes the secret value as plain UTF-8.
    /// Returns actual bytes written, or 0 if not found, or negative on error.
    fn secrets_get(key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32) -> i32;
}

// ── Safe Rust wrappers ────────────────────────────────────────────────────────

/// Emit an info-level log line.
fn flux_log(message: &str) {
    unsafe { log(1, message.as_ptr() as i32, message.len() as i32) }
}

/// Execute a SQL query. Returns the JSON response string on success.
fn flux_db_query(sql: &str, params: &str) -> Result<String, String> {
    let mut out = vec![0u8; 65536];
    let n = unsafe {
        db_query(
            sql.as_ptr() as i32, sql.len() as i32,
            params.as_ptr() as i32, params.len() as i32,
            out.as_mut_ptr() as i32, out.len() as i32,
        )
    };
    if n < 0 {
        let msg = std::str::from_utf8(&out[..(-n) as usize]).unwrap_or("db_query error").to_string();
        return Err(msg);
    }
    Ok(std::str::from_utf8(&out[..n as usize]).unwrap_or("[]").to_string())
}

/// Push an async job. Returns the job ID on success.
fn flux_queue_push(function_name: &str, payload: &str, delay: Option<&str>) -> Result<String, String> {
    let req = match delay {
        Some(d) => format!(r#"{{"function":"{function_name}","payload":{payload},"delay":"{d}"}}"#),
        None    => format!(r#"{{"function":"{function_name}","payload":{payload}}}"#),
    };
    let mut out = vec![0u8; 4096];
    let n = unsafe {
        queue_push(req.as_ptr() as i32, req.len() as i32, out.as_mut_ptr() as i32, out.len() as i32)
    };
    if n < 0 {
        let msg = std::str::from_utf8(&out[..(-n) as usize]).unwrap_or("queue_push error").to_string();
        return Err(msg);
    }
    Ok(std::str::from_utf8(&out[..n as usize]).unwrap_or("{}").to_string())
}

// ── Handler ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct Input {
    // TODO: add your input fields
    // Example: pub user_id: String,
}

#[derive(Serialize)]
pub struct Output {
    ok: bool,
}

// ── Required exports ──────────────────────────────────────────────────────────

/// Allocate `size` bytes on the heap for the host to write the incoming payload.
/// Leaking is safe — the Wasmtime Store is dropped after each invocation.
#[no_mangle]
pub extern "C" fn __flux_alloc(size: i32) -> i32 {
    let mut v = Vec::<u8>::with_capacity(size as usize);
    let ptr = v.as_mut_ptr() as i32;
    std::mem::forget(v);
    ptr
}

/// Main entry point called by the Flux runtime.
/// Returns a pointer to: [ u32 LE byte-length ][ JSON bytes ].
#[no_mangle]
pub extern "C" fn handle(ptr: i32, len: i32) -> i32 {
    let input_bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };

    let _input: Input = match serde_json::from_slice(input_bytes) {
        Ok(v)  => v,
        Err(e) => return write_result(&serde_json::json!({ "error": e.to_string() })),
    };

    flux_log("hello handler invoked");

    // ── Database example ─────────────────────────────────────────────────
    // let rows = flux_db_query("SELECT id FROM users WHERE active = $1", "[true]")
    //     .unwrap_or_else(|e| { flux_log(&format!("db: {e}")); "[]".into() });

    // ── Queue example ────────────────────────────────────────────────────
    // flux_queue_push("send_welcome_email", r#"{"userId":"123"}"#, None)
    //     .unwrap_or_else(|e| { flux_log(&format!("q: {e}")); "{}".into() });

    let output = Output { ok: true };
    write_result(&serde_json::json!({ "output": output }))
}

/// Serialize `value` as `[u32 LE length][JSON bytes]`, leak the buffer, return pointer.
fn write_result(value: &serde_json::Value) -> i32 {
    let json = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(&json);
    let ptr = buf.as_ptr() as i32;
    std::mem::forget(buf);
    ptr
}
