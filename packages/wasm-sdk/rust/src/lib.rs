//! # flux-wasm-sdk
//!
//! Rust SDK for writing Flux functions that compile to WebAssembly.
//!
//! ## Quick start
//!
//! ```toml
//! # Cargo.toml
//! [package]
//! name = "my-function"
//! version = "0.1.0"
//! edition = "2021"
//!
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! flux-wasm-sdk = "0.1"
//! serde = { version = "1", features = ["derive"] }
//! serde_json = "1"
//! ```
//!
//! ```rust
//! use flux_wasm_sdk::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Deserialize)]
//! struct Input { name: String }
//!
//! #[derive(Serialize)]
//! struct Output { message: String }
//!
//! register_handler!(handler);
//!
//! fn handler(ctx: &FluxCtx, input: Input) -> FluxResult<Output> {
//!     ctx.log_info(&format!("called with name={}", input.name));
//!     let api_key = ctx.secrets.get("MY_API_KEY");
//!     Ok(Output { message: format!("Hello, {}!", input.name) })
//! }
//! ```
//!
//! ## Build
//!
//! ```bash
//! rustup target add wasm32-wasip1
//! cargo build --target wasm32-wasip1 --release
//! # Optional: optimise binary size
//! wasm-opt -Oz target/wasm32-wasip1/release/my_function.wasm -o handler.wasm
//! ```
//!
//! ## Deploy
//!
//! ```bash
//! flux deploy   # reads flux.json, uploads handler.wasm
//! ```

use std::collections::HashMap;

// ─── Host imports (fluxbase namespace) ──────────────────────────────────────
//
// These are provided by the Flux runtime host and linked at instantiation time.
// The `#[link(wasm_import_module = "fluxbase")]` attribute tells the WASM linker
// to import them from the "fluxbase" namespace (not the default "env" namespace).

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "fluxbase")]
extern "C" {
    /// Emit a structured log line.
    /// level: 0=debug, 1=info, 2=warn, 3=error
    fn log(level: i32, msg_ptr: i32, msg_len: i32);

    /// Look up a secret by name.
    /// Writes the value into `out_ptr` (max `out_max` bytes).
    /// Returns actual byte length written, or -1 if the key is not found.
    fn secrets_get(key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32) -> i32;

    /// Outbound HTTP request gated by the host allow-list.
    /// `req_ptr` points to a JSON request object:
    ///   `{"method":"GET","url":"https://...","headers":{},"body":"<base64>"}`
    /// `out_ptr` receives a JSON response object:
    ///   `{"status":200,"headers":{},"body":"<base64>"}`
    /// Returns bytes written into `out_ptr`, or -1 if denied / error.
    fn http_fetch(req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32) -> i32;
}

// ─── Stub implementations for non-WASM builds (tests, docs) ─────────────────

#[cfg(not(target_arch = "wasm32"))]
unsafe fn log(_level: i32, _msg_ptr: i32, _msg_len: i32) {}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn secrets_get(_key_ptr: i32, _key_len: i32, _out_ptr: i32, _out_max: i32) -> i32 { -1 }

#[cfg(not(target_arch = "wasm32"))]
unsafe fn http_fetch(_rq_ptr: i32, _rq_len: i32, _out_ptr: i32, _out_max: i32) -> i32 { -1 }

// ─── Global result buffer ────────────────────────────────────────────────────
//
// The `handle` export must return a pointer into the module's linear memory.
// We use a static Vec<u8> so the host can read the result after the call returns.

static mut RESULT_BUFFER: Vec<u8> = Vec::new();

// ─── Allocator export ────────────────────────────────────────────────────────

/// Required export: the host calls this to allocate `size` bytes in the module's
/// heap before writing the payload JSON.  Returns a pointer or 0 on failure.
#[no_mangle]
pub extern "C" fn __flux_alloc(size: i32) -> i32 {
    let layout = match std::alloc::Layout::array::<u8>(size as usize) {
        Ok(l) => l,
        Err(_) => return 0,
    };
    // SAFETY: layout is valid (array of u8)
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() { 0 } else { ptr as i32 }
}

/// Optional export: the host can call this to free a payload allocation.
#[no_mangle]
pub extern "C" fn __flux_free(ptr: i32, size: i32) {
    if ptr == 0 || size <= 0 { return; }
    let layout = match std::alloc::Layout::array::<u8>(size as usize) {
        Ok(l) => l,
        Err(_) => return,
    };
    // SAFETY: ptr was allocated by __flux_alloc with the same layout
    unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
}

// ─── FluxSecrets ─────────────────────────────────────────────────────────────

/// Access to named secrets injected by the Flux runtime.
pub struct FluxSecrets;

impl FluxSecrets {
    /// Fetch a secret by name.  Returns `None` if the key does not exist.
    pub fn get(&self, key: &str) -> Option<String> {
        let mut buf = vec![0u8; 8192];
        let len = unsafe {
            secrets_get(
                key.as_ptr() as i32,
                key.len() as i32,
                buf.as_mut_ptr() as i32,
                buf.len() as i32,
            )
        };
        if len < 0 {
            None
        } else {
            buf.truncate(len as usize);
            String::from_utf8(buf).ok()
        }
    }
}

// ─── FluxCtx ─────────────────────────────────────────────────────────────────

/// Context passed to every function handler.
///
/// Provides access to secrets and structured logging.
pub struct FluxCtx {
    pub secrets: FluxSecrets,
}

impl FluxCtx {
    /// Emit a debug log line.
    pub fn log_debug(&self, message: &str) { self.log(0, message); }
    /// Emit an info log line.
    pub fn log_info(&self, message: &str) { self.log(1, message); }
    /// Emit a warning log line.
    pub fn log_warn(&self, message: &str) { self.log(2, message); }
    /// Emit an error log line.
    pub fn log_error(&self, message: &str) { self.log(3, message); }

    /// Emit a log line at `level` (0=debug, 1=info, 2=warn, 3=error).
    pub fn log(&self, level: i32, message: &str) {
        unsafe {
            log(level, message.as_ptr() as i32, message.len() as i32);
        }
    }

    /// Make an outbound HTTP request.
    ///
    /// The host enforces an allow-list (`WASM_HTTP_ALLOWED_HOSTS`). Requests to
    /// hosts not on the list return `None`.
    ///
    /// # Example
    /// ```rust
    /// let resp = ctx.http_fetch("GET", "https://api.example.com/data", None, None);
    /// ```
    pub fn http_fetch(
        &self,
        method:  &str,
        url:     &str,
        headers: Option<std::collections::HashMap<&str, &str>>,
        body:    Option<&[u8]>,
    ) -> Option<HttpResponse> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let body_b64 = body.map(|b| STANDARD.encode(b)).unwrap_or_default();
        let headers_val: serde_json::Value = headers
            .map(|h| h.into_iter().map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string()))).collect())
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let req = serde_json::json!({
            "method":  method,
            "url":     url,
            "headers": headers_val,
            "body":    body_b64,
        });
        let req_bytes = serde_json::to_vec(&req).ok()?;
        let mut out_buf = vec![0u8; 128 * 1024]; // 128 KB response buffer

        let len = unsafe {
            http_fetch(
                req_bytes.as_ptr() as i32,
                req_bytes.len() as i32,
                out_buf.as_mut_ptr() as i32,
                out_buf.len() as i32,
            )
        };
        if len < 0 { return None; }
        out_buf.truncate(len as usize);

        let resp: serde_json::Value = serde_json::from_slice(&out_buf).ok()?;
        let status  = resp.get("status").and_then(|s| s.as_u64()).map(|s| s as u16).unwrap_or(0);
        let body_b64 = resp.get("body").and_then(|b| b.as_str()).unwrap_or("");
        let body_bytes = STANDARD.decode(body_b64).unwrap_or_default();
        let resp_headers = resp.get("headers")
            .and_then(|h| h.as_object())
            .map(|h| h.iter().filter_map(|(k, v)| v.as_str().map(|vs| (k.clone(), vs.to_string()))).collect())
            .unwrap_or_default();
        Some(HttpResponse { status, headers: resp_headers, body: body_bytes })
    }
}
// ─── HttpResponse ────────────────────────────────────────────────────────────────────

/// Response returned by [`FluxCtx::http_fetch`].
pub struct HttpResponse {
    pub status:  u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body:    Vec<u8>,
}

impl HttpResponse {
    /// Decode the body as UTF-8 text.
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.body).ok()
    }
    /// Parse the body as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_slice(&self.body).ok()
    }
}
// ─── FluxResult ──────────────────────────────────────────────────────────────

/// Return type for Flux WASM handlers.
///
/// `Ok(output)` is serialised to JSON and returned to the caller.
/// `Err(message)` is returned as `{ "error": "message" }`.
pub type FluxResult<T> = Result<T, String>;

// ─── Internal: write result to global buffer ─────────────────────────────────

/// Write `[4 bytes LE length][JSON bytes]` to `RESULT_BUFFER` and return the
/// pointer.  Called by the generated `handle` export.
pub fn write_result_buffer(json: &[u8]) -> i32 {
    let len = json.len() as u32;
    unsafe {
        RESULT_BUFFER.clear();
        RESULT_BUFFER.reserve(4 + json.len());
        RESULT_BUFFER.extend_from_slice(&len.to_le_bytes());
        RESULT_BUFFER.extend_from_slice(json);
        RESULT_BUFFER.as_ptr() as i32
    }
}

// ─── register_handler! macro ─────────────────────────────────────────────────

/// Register a Rust function as the Flux WASM handler.
///
/// Usage:
/// ```rust
/// use flux_wasm_sdk::prelude::*;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize)]
/// struct Input { name: String }
///
/// #[derive(Serialize)]
/// struct Output { message: String }
///
/// register_handler!(my_fn);
///
/// fn my_fn(ctx: &FluxCtx, input: Input) -> FluxResult<Output> {
///     Ok(Output { message: format!("Hello, {}!", input.name) })
/// }
/// ```
///
/// The macro generates the `handle` export:
/// - Reads the payload JSON bytes written by the host at `payload_ptr`
/// - Deserialises them into `Input` using `serde_json`
/// - Calls your function with a `&FluxCtx` and the parsed `Input`
/// - Serialises the result to JSON and writes it to the result buffer
/// - Returns a pointer to `[4-byte len][JSON]` for the host to read
#[macro_export]
macro_rules! register_handler {
    // register_handler!(fn_name) — deserialises the entire payload as Input
    ($handler:ident) => {
        #[no_mangle]
        pub extern "C" fn handle(payload_ptr: i32, payload_len: i32) -> i32 {
            use $crate::write_result_buffer;
            // Read payload bytes from linear memory (written by host).
            let payload_bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(payload_ptr as *const u8, payload_len as usize)
            };

            // Deserialise payload JSON into the handler's Input type.
            let input = match $crate::__parse_input(payload_bytes) {
                Ok(v) => v,
                Err(e) => {
                    let err = format!(r#"{{"error":"input deserialization failed: {}"}}"#, e);
                    return write_result_buffer(err.as_bytes());
                }
            };

            // Build the ctx object.
            let ctx = $crate::FluxCtx { secrets: $crate::FluxSecrets };

            // Call the user's handler.
            match $handler(&ctx, input) {
                Ok(output) => {
                    match $crate::__serialize_output(&output) {
                        Ok(json) => write_result_buffer(json.as_bytes()),
                        Err(e) => {
                            let err = format!(r#"{{"error":"output serialization failed: {}"}}"#, e);
                            write_result_buffer(err.as_bytes())
                        }
                    }
                }
                Err(message) => {
                    let err = format!(r#"{{"error":{}}}"#, serde_json::to_string(&message).unwrap_or_default());
                    write_result_buffer(err.as_bytes())
                }
            }
        }
    };
}

// ─── Helpers used by the macro (pub so the macro can reference them) ──────────

#[doc(hidden)]
pub fn __parse_input<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

#[doc(hidden)]
pub fn __serialize_output<T: serde::Serialize>(output: &T) -> Result<String, String> {
    let inner = serde_json::to_value(output).map_err(|e| e.to_string())?;
    serde_json::to_string(&serde_json::json!({ "output": inner })).map_err(|e| e.to_string())
}

// ─── Prelude ─────────────────────────────────────────────────────────────────

pub mod prelude {
    pub use crate::{FluxCtx, FluxResult, FluxSecrets, HttpResponse, register_handler};
    pub use serde::{Deserialize, Serialize};
}
