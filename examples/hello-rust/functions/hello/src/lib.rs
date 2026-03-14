use serde::{Deserialize, Serialize};

// ── Fluxbase host imports ─────────────────────────────────────────────────────
//
// The Flux runtime exposes these functions to every WASM module.
// All I/O is async at the OS level: the WASM fiber suspends while the runtime
// handles the call, so no OS threads are blocked.
//
// Memory convention:
//   - Input : pass a pointer + byte length into the module's linear memory.
//   - Output: pass a pre-allocated buffer (ptr + max_len). The host writes the
//             result as a UTF-8 JSON string and returns the actual byte count.
//             A negative return value signals an error; read the buffer for the
//             error message.
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

/// Flux calls this function with JSON-encoded input.
///
/// Memory layout:
///   - `ptr` points to `input_bytes` inside the module's linear memory.
///   - Returns `(out_ptr << 32) | out_len` — caller reads `out_len` bytes at `out_ptr`.
#[no_mangle]
pub extern "C" fn hello_handler(ptr: i32, len: i32) -> i64 {
    let input_bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let _input: Input = serde_json::from_slice(input_bytes).unwrap();

    flux_log("hello_handler invoked");

    // ── Database example ─────────────────────────────────────────────────
    // let rows_json = flux_db_query(
    //     "SELECT id, name FROM users WHERE active = $1",
    //     "[true]",
    // ).unwrap_or_else(|e| { flux_log(&format!("db error: {e}")); "[]".into() });

    // ── Queue example ────────────────────────────────────────────────────
    // flux_queue_push("send_welcome_email", r#"{"userId":"123"}"#, None)
    //     .unwrap_or_else(|e| { flux_log(&format!("queue error: {e}")); "{}".into() });

    let output = Output { ok: true };
    let out_bytes = serde_json::to_vec(&output).unwrap();

    let out_ptr = out_bytes.as_ptr() as i64;
    let out_len = out_bytes.len() as i64;
    std::mem::forget(out_bytes);
    (out_ptr << 32) | out_len
}
