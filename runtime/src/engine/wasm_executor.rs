//! WASM execution engine — runs `.wasm` modules compiled by Wasmtime (Cranelift).
//!
//! ## ABI contract with the WASM module
//!
//! ### Host imports the module must accept (`flux` namespace):
//!
//! | Function | Signature | Behaviour |
//! |---|---|---|
//! | `flux.log` | `(level: i32, msg_ptr: i32, msg_len: i32)` | Emit a structured log line |
//! | `flux.secrets_get` | `(key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32) → i32` | Read a secret; returns actual byte length or -1 if missing |
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
//! ```text
//! [ u32 LE length ][ <length> bytes of UTF-8 JSON ]
//! ```
//! The JSON must have either `"output"` or `"error"` keys at the top level.
//! Logs are emitted via `flux.log` during execution, not in the result.

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
//! ## Host imports (`flux` namespace)
//!
//! WASM modules may call the following host functions via linear memory pointers:
//!
//! | Import | Purpose |
//! |---|---|
//! | `flux.log(level, ptr, len)` | Append a `LogLine` to `HostState::logs` |
//! | `flux.secrets_get(key_ptr, key_len, out_ptr, out_max) → i32` | Copy a secret value into WASM memory; returns byte length or -1 |
//! | `flux.http_fetch(...)` | Outbound HTTP (allow-listed hosts only) |
//!
//! ## Memory safety
//!
//! All pointer/length pairs received from WASM are bounds-checked against
//! `memory.data_size()` before slicing. Invalid pointers return an error rather than
//! panicking the host process.
use std::collections::HashMap;
use wasmtime::{
    Caller, Config, Engine, Linker, Module, OptLevel, Store, Val,
};
use tokio::time::{timeout, Duration};

use crate::engine::executor::{ExecutionResult, LogLine, PoolDispatchers};
// ─── HostState ─────────────────────────────────────────────────────────────

/// Data owned by the Wasmtime `Store` — accessible from host import callbacks.
pub struct HostState {
    pub secrets:            HashMap<String, String>,
    pub logs:               Vec<LogLine>,
    /// `http_fetch` allow-list.  Empty vec = deny all.  Contains `"*"` = allow all.
    pub allowed_http_hosts: Vec<String>,
    /// Shared reqwest client for outbound HTTP from `flux.http_fetch`.
    pub http_client:        reqwest::Client,
    /// Project Postgres schema name (e.g. `project_abc123`).
    pub database:           String,
    /// Dispatch traits for in-process data-engine, queue, and API calls.
    pub dispatchers:        PoolDispatchers,
    /// WASI stdin buffer — populated before calling `_start` in command-model modules.
    pub stdin_buf:          Vec<u8>,
    /// Current read position in `stdin_buf`.
    pub stdin_pos:          usize,
    /// WASI stdout capture — populated by fd_write(fd=1) calls from the WASM module.
    pub stdout_buf:         Vec<u8>,
    /// WASI stderr capture — populated by fd_write(fd=2) calls (e.g. Go panic messages).
    pub stderr_buf:         Vec<u8>,
    /// WASI argv — command-line arguments returned by args_get/args_sizes_get.
    /// Empty = no arguments (default for self-contained WASM like Go, Rust, AssemblyScript).
    /// For interpreter WASM (PHP: ["php", "-r", "<code>"]) these are set from
    /// the `flux.wasi-args` custom section embedded in the binary at deploy time.
    pub wasi_argv:          Vec<String>,
}

// ─── Params ────────────────────────────────────────────────────────────────

pub struct WasmExecutionParams {
    pub secrets:      HashMap<String, String>,
    /// Optional WASI command-line arguments for interpreter-style WASM modules.
    /// Parsed from the `flux.wasi-args` custom section of the WASM binary.
    /// Empty = no args (default; all self-contained WASM functions like Go/Rust/AS).
    pub wasi_argv:    Vec<String>,
    pub payload:      serde_json::Value,
    /// Maximum WASM CPU fuel (instructions).  1 billion ≈ a few hundred ms.
    pub fuel_limit:   u64,
    /// Hosts the WASM function is allowed to call via `flux.http_fetch`.
    /// Empty = deny all.  `["*"]` = allow all (use with caution).
    pub allowed_http_hosts: Vec<String>,
    /// Shared HTTP client passed through for outbound calls.
    pub http_client: Option<reqwest::Client>,
    /// Per-request wall-clock timeout in seconds.
    pub timeout_secs: u64,
    /// Project Postgres schema name (e.g. `project_abc123`).
    pub database: String,
    /// Dispatch traits for in-process data-engine, queue, and API calls.
    pub dispatchers: PoolDispatchers,
}

impl WasmExecutionParams {
    /// Create params with default values for testing.
    /// Requires a `PoolDispatchers` since there's no meaningful default.
    #[cfg(test)]
    pub fn test_default(dispatchers: PoolDispatchers) -> Self {
        Self {
            secrets:            HashMap::new(),
            payload:            serde_json::Value::Null,
            fuel_limit:         1_000_000_000,
            wasi_argv:          Vec::new(),
            allowed_http_hosts: Vec::new(),
            http_client:        None,
            timeout_secs:       30,
            database:           String::new(),
            dispatchers,
        }
    }
}

// ─── Engine factory ────────────────────────────────────────────────────────

/// Build a shared Wasmtime `Engine` with Cranelift AOT + fuel interruption + async support.
///
/// `async_support(true)` enables fiber-based suspension of WASM execution during
/// host I/O calls. When a WASM module calls `flux.http_fetch`, the fiber is
/// suspended and tokio can drive other pending Futures (DB queries, HTTP calls from
/// other concurrent WASM executions) until the response arrives.
pub fn build_engine() -> Engine {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    cfg.async_support(true);
    Engine::new(&cfg).expect("failed to build Wasmtime engine")
}

/// Build a Wasmtime `Engine` with `OptLevel::None` for interpreter-style WASM binaries.
///
/// Large interpreter runtimes (PHP 13 MB, Python 30 MB, Ruby 50 MB) contain huge
/// functions (e.g. PHP's parser dispatch loop at ~207 KB of WASM bytecode) that
/// cause Cranelift's register allocator to take pathologically long with the default
/// `OptLevel::Speed`.  `OptLevel::None` skips most optimisation passes and reduces
/// compilation time from tens-of-minutes to a few seconds for these binaries.
///
/// The runtime perf difference for interpreter WASM is negligible — the interpreter
/// loop dominates execution time regardless of Cranelift codegen quality.
pub fn build_engine_fast() -> Engine {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    cfg.async_support(true);
    cfg.cranelift_opt_level(OptLevel::None);
    Engine::new(&cfg).expect("failed to build Wasmtime fast engine")
}

// ─── Core execution ────────────────────────────────────────────────────────

/// Compile a WASM module from raw bytes using the shared engine.
/// This is the expensive step (~5–50 ms); results should be cached.
pub fn compile_module(engine: &Engine, bytes: &[u8]) -> Result<Module, String> {
    Module::from_binary(engine, bytes)
        .map_err(|e| format!("wasm compilation failed: {}", e))
}

/// Parse the `flux.wasi-args` custom WASM section from raw bytes.
///
/// The section contains NUL-separated argument strings, e.g.:
///   `php\0-r\0<?php echo json_encode(['ok'=>true]);`
///
/// This is used for interpreter-style WASM bundles (PHP, future Ruby CLI builds)
/// where the interpreter binary is distributed separately and the user script is
/// embedded at deploy time by the Flux CLI.
///
/// Returns an empty Vec if the section is absent (self-contained WASM like Go,
/// Rust, AssemblyScript — they don't need WASI args).
pub fn parse_wasi_args(bytes: &[u8]) -> Vec<String> {
    // WASM binary: magic(4) + version(4) then sections.
    // Custom section: id=0, size(leb128), name_len(leb128), name, content.
    if bytes.len() < 8 { return Vec::new(); }
    let mut pos = 8usize;
    while pos < bytes.len() {
        let section_id = bytes[pos]; pos += 1;
        // Read LEB128 section size
        let mut size: usize = 0; let mut shift = 0;
        loop {
            if pos >= bytes.len() { return Vec::new(); }
            let b = bytes[pos]; pos += 1;
            size |= ((b & 0x7f) as usize) << shift; shift += 7;
            if b & 0x80 == 0 { break; }
        }
        let section_end = pos + size;
        if section_id == 0 {
            // Custom section — read name
            let mut name_len: usize = 0; let mut shift = 0;
            let mut p = pos;
            loop {
                if p >= section_end { break; }
                let b = bytes[p]; p += 1;
                name_len |= ((b & 0x7f) as usize) << shift; shift += 7;
                if b & 0x80 == 0 { break; }
            }
            if p + name_len <= section_end {
                let name = &bytes[p..p + name_len];
                if name == b"flux.wasi-args" {
                    let content_start = p + name_len;
                    let content = &bytes[content_start..section_end];
                    // Split on NUL bytes; decode each as UTF-8
                    return content.split(|&b| b == 0)
                        .filter(|s| !s.is_empty())
                        .map(|s| String::from_utf8_lossy(s).into_owned())
                        .collect();
                }
            }
        }
        pos = section_end;
    }
    Vec::new()
}

/// Execute a pre-compiled `Module`.
///
/// Uses `spawn_blocking` so that CPU-bound WASM execution (Python, Ruby, Go
/// command-model modules whose host imports are all synchronous) does not starve
/// the tokio worker pool.  The runtime `Handle` lets async host imports
/// (`http_fetch`, `db_query`) still schedule on the existing runtime from within
/// the blocking thread via `Handle::block_on`.
pub async fn execute_wasm(
    engine: &Engine,
    module: &Module,
    params: WasmExecutionParams,
) -> Result<ExecutionResult, String> {
    let engine = engine.clone();
    let module = module.clone();
    let timeout_secs = params.timeout_secs;

    // spawn_blocking runs on a separate OS-thread pool that does not interfere
    // with tokio's async worker threads.  This fixes thread starvation when
    // large WASM modules (Python 26 MB, Ruby 47 MB) execute for 30-60 s.
    let rt = tokio::runtime::Handle::current();
    let handle = tokio::task::spawn_blocking(move || {
        rt.block_on(execute_wasm_async(&engine, &module, params))
    });

    match timeout(Duration::from_secs(timeout_secs + 5), handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(join_err)) => Err(format!("wasm worker panicked: {}", join_err)),
        Err(_) => Err(format!("wasm execution timed out after {} seconds", timeout_secs)),
    }
}

// ─── Async kernel (tokio task; fibers yield during host I/O) ─────────────────

async fn execute_wasm_async(
    engine: &Engine,
    module: &Module,
    params: WasmExecutionParams,
) -> Result<ExecutionResult, String> {
    let host = HostState {
        secrets:            params.secrets,
        logs:               Vec::new(),
        allowed_http_hosts: params.allowed_http_hosts,
        http_client:        params.http_client.unwrap_or_else(reqwest::Client::new),
        database:           params.database,
        dispatchers:        params.dispatchers,
        stdin_buf:          Vec::new(),
        stdin_pos:          0,
        stdout_buf:         Vec::new(),
        stderr_buf:         Vec::new(),
        wasi_argv:          params.wasi_argv,
    };

    let mut store = Store::new(engine, host);
    store.set_fuel(params.fuel_limit)
        .map_err(|e| format!("fuel setup error: {}", e))?;

    // ── Register host imports ──────────────────────────────────────────────

    let mut linker = Linker::<HostState>::new(engine);

    // env.abort(msg: i32, file: i32, line: i32, col: i32)
    //
    // AssemblyScript, C/emscripten, and several other toolchains generate an
    // `env.abort` import as a panicking hook.  We provide a no-op stub so the
    // module instantiates; if the function is ever called at runtime the WASM
    // execution will trap (via the Wasmtime fuel/trap mechanism) before
    // corrupting state.
    linker.func_wrap("env", "abort", |_: Caller<HostState>, _msg: i32, _file: i32, _line: i32, _col: i32| {
        // Intentional no-op stub.  Abort in a WASM module is fatal to the
        // isolate, not the host — return normally and let the module trap.
    }).map_err(|e| e.to_string())?;

    // ── WASI stubs ──────────────────────────────────────────────────────────────
    //
    // Go (GOOS=wasip1) and C (wasi-sdk) modules import WASI host functions.
    // The Flux runtime does not implement a real WASI environment, so these are
    // no-op / minimal-viable stubs that let the module instantiate and run the
    // exported `handle` function.  Calls that would interact with the OS (e.g.
    // fd_write to stdout) are silently discarded; proc_exit terminates execution
    // by trapping the isolate (handled by Wasmtime fuel limits).

    // sched_yield() -> i32
    linker.func_wrap("wasi_snapshot_preview1", "sched_yield",
        |_: Caller<HostState>| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // proc_exit(code: i32)
    // Trap with a recognizable sentinel so _start stops cleanly after writing stdout.
    linker.func_wrap("wasi_snapshot_preview1", "proc_exit",
        |_: Caller<HostState>, code: i32| -> Result<(), anyhow::Error> {
            anyhow::bail!("__wasi_proc_exit:{}", code)
        }
    ).map_err(|e| e.to_string())?;

    // args_get(argv: i32, argv_buf: i32) -> i32
    //
    // Write argv pointers and the NUL-terminated argument strings into WASM memory.
    // Layout:
    //   argv[0..argc] at argv_ptr: array of i32 pointers into argv_buf
    //   argv_buf: NUL-terminated strings packed sequentially
    //
    // When wasi_argv is empty (default for self-contained WASM like Go, Rust) we
    // return argc=0 and buf_size=0 — the module sees an empty argv as expected.
    linker.func_wrap("wasi_snapshot_preview1", "args_get",
        |mut caller: Caller<HostState>, argv_ptr: i32, argv_buf_ptr: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let argv = caller.data().wasi_argv.clone();
            if argv.is_empty() {
                return 0;
            }
            // Write each arg as a NUL-terminated string into argv_buf, record pointer.
            let mut buf_offset = argv_buf_ptr as usize;
            for (i, arg) in argv.iter().enumerate() {
                let ptr_offset = argv_ptr as usize + i * 4;
                let _ = mem.write(&mut caller, ptr_offset, &(buf_offset as u32).to_le_bytes());
                let bytes = arg.as_bytes();
                let _ = mem.write(&mut caller, buf_offset, bytes);
                buf_offset += bytes.len();
                let _ = mem.write(&mut caller, buf_offset, &[0u8]); // NUL
                buf_offset += 1;
            }
            0
        }
    ).map_err(|e| e.to_string())?;

    // args_sizes_get(argc_out: i32, argv_buf_size_out: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "args_sizes_get",
        |mut caller: Caller<HostState>, argc_out: i32, argv_buf_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let argv = &caller.data().wasi_argv;
            let argc = argv.len() as u32;
            let buf_size: u32 = argv.iter().map(|a| a.len() as u32 + 1).sum(); // +1 for NUL
            let _ = mem.write(&mut caller, argc_out as usize, &argc.to_le_bytes());
            let _ = mem.write(&mut caller, argv_buf_out as usize, &buf_size.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // clock_time_get(id: i32, precision: i64, time_out: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "clock_time_get",
        |mut caller: Caller<HostState>, _id: i32, _prec: i64, out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            // Return real nanoseconds since UNIX epoch — Go asserts nanotime() != 0.
            let nanos: u64 = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1_700_000_000_000_000_000); // fallback: ~Nov 2023
            let _ = mem.write(&mut caller, out as usize, &nanos.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // clock_res_get(id: i32, res_out: i32) -> i32
    // Returns the resolution of a clock. Used by Python/Nuitka WASM.
    linker.func_wrap("wasi_snapshot_preview1", "clock_res_get",
        |mut caller: Caller<HostState>, _id: i32, res_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            // Return 1 nanosecond resolution.
            let res: u64 = 1;
            let _ = mem.write(&mut caller, res_out as usize, &res.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // environ_get(environ: i32, environ_buf: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "environ_get",
        |_: Caller<HostState>, _environ: i32, _environ_buf: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // environ_sizes_get(count_out: i32, size_out: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "environ_sizes_get",
        |mut caller: Caller<HostState>, count_out: i32, size_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let _ = mem.write(&mut caller, count_out as usize, &0u32.to_le_bytes());
            let _ = mem.write(&mut caller, size_out as usize, &0u32.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_write(fd: i32, iovs: i32, iovs_len: i32, nwritten: i32) -> i32
    // fd=1 (stdout) is captured into host.stdout_buf; all other fds are discarded.
    linker.func_wrap("wasi_snapshot_preview1", "fd_write",
        |mut caller: Caller<HostState>, fd: i32, iovs: i32, iovs_len: i32, nwritten: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8, // EBADF
            };
            let mut total: u32 = 0;
            let mut captured: Vec<u8> = Vec::new();
            for i in 0..iovs_len as usize {
                let base = iovs as usize + i * 8;
                let mut ptr_bytes = [0u8; 4];
                let mut len_bytes = [0u8; 4];
                if mem.read(&caller, base, &mut ptr_bytes).is_err() { break; }
                if mem.read(&caller, base + 4, &mut len_bytes).is_err() { break; }
                let buf_ptr = u32::from_le_bytes(ptr_bytes) as usize;
                let buf_len = u32::from_le_bytes(len_bytes) as usize;
                total += buf_len as u32;
                if fd == 1 || fd == 2 {
                    let data = mem.data(&caller);
                    if buf_ptr + buf_len <= data.len() {
                        captured.extend_from_slice(&data[buf_ptr..buf_ptr + buf_len]);
                    }
                }
            }
            if fd == 1 && !captured.is_empty() {
                caller.data_mut().stdout_buf.extend(captured);
            } else if fd == 2 && !captured.is_empty() {
                caller.data_mut().stderr_buf.extend(captured);
            }
            let _ = mem.write(&mut caller, nwritten as usize, &total.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_read(fd: i32, iovs: i32, iovs_len: i32, nread: i32) -> i32
    // fd=0 (stdin) is served from host.stdin_buf; all other fds return EBADF.
    linker.func_wrap("wasi_snapshot_preview1", "fd_read",
        |mut caller: Caller<HostState>, fd: i32, iovs: i32, iovs_len: i32, nread_ptr: i32| -> i32 {
            if fd != 0 {
                return 8; // EBADF
            }
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let stdin_len = caller.data().stdin_buf.len();
            let stdin_pos = caller.data().stdin_pos;
            let mut total_read: u32 = 0;
            for i in 0..iovs_len as usize {
                let iov_base = iovs as usize + i * 8;
                let mut ptr_bytes = [0u8; 4];
                let mut len_bytes = [0u8; 4];
                if mem.read(&caller, iov_base, &mut ptr_bytes).is_err() { break; }
                if mem.read(&caller, iov_base + 4, &mut len_bytes).is_err() { break; }
                let buf_ptr = u32::from_le_bytes(ptr_bytes) as usize;
                let buf_len = u32::from_le_bytes(len_bytes) as usize;
                let src_start = stdin_pos + total_read as usize;
                let remaining = stdin_len.saturating_sub(src_start);
                let to_copy = remaining.min(buf_len);
                if to_copy > 0 {
                    let chunk: Vec<u8> = caller.data().stdin_buf[src_start..src_start + to_copy].to_vec();
                    if mem.write(&mut caller, buf_ptr, &chunk).is_err() { break; }
                    total_read += to_copy as u32;
                }
            }
            caller.data_mut().stdin_pos += total_read as usize;
            let _ = mem.write(&mut caller, nread_ptr as usize, &total_read.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // random_get(buf: i32, buf_len: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "random_get",
        |mut caller: Caller<HostState>, buf: i32, buf_len: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            // Fill with pseudo-random bytes using a simple xorshift seeded from pointer.
            let mut state = buf as u32 ^ 0x9e3779b9;
            let data = mem.data_mut(&mut caller);
            let start = buf as usize;
            let end = start.saturating_add(buf_len as usize).min(data.len());
            for byte in &mut data[start..end] {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            0
        }
    ).map_err(|e| e.to_string())?;

    // poll_oneoff(in: i32, out: i32, nsubscriptions: i32, nevents_ptr: i32) -> i32
    //
    // WASI subscription layout (48 bytes each):
    //   offset 0:  userdata (u64, 8 bytes)
    //   offset 8:  tag (u8)  — 0=CLOCK, 1=FD_READ, 2=FD_WRITE
    //   offset 9:  padding (7 bytes)
    //   offset 16: union — for FD_READ/FD_WRITE: fd (u32); for CLOCK: clockid (u32)
    //
    // WASI event layout (32 bytes each):
    //   offset 0:  userdata (u64)
    //   offset 8:  error (u16) — 0 = success
    //   offset 10: type (u8)  — same tag values
    //   offset 11: padding (5 bytes)
    //   offset 16: nbytes (u64) for FD_READ/FD_WRITE (bytes available)
    //   offset 24: flags (u16) for FD_READ/FD_WRITE
    //   offset 26: padding (6 bytes)
    //
    // Go's wasip1 goroutine scheduler calls poll_oneoff to wait for I/O events.
    // When our stub returned 0 events, Go entered a busy-spin loop checking
    // stdin readiness, exhausting the fuel budget.  We now properly report:
    //   - FD_READ(fd=0): ready when stdin_buf has unread data; EOF otherwise
    //   - FD_WRITE(fd=1/2): always ready
    //   - CLOCK: always elapsed (we have no real timer; treat all deadlines as past)
    //   - Fallback: if no subscription matched, report a synthetic CLOCK event so
    //     Go's scheduler yields instead of spinning.
    linker.func_wrap("wasi_snapshot_preview1", "poll_oneoff",
        |mut caller: Caller<HostState>, in_ptr: i32, out_ptr: i32, nsubs: i32, nevents_ptr: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            if nsubs <= 0 {
                let _ = mem.write(&mut caller, nevents_ptr as usize, &0u32.to_le_bytes());
                return 0;
            }
            let stdin_has_data = {
                let d = caller.data();
                d.stdin_buf.len() > d.stdin_pos
            };
            let mut out_buf: Vec<u8> = Vec::new();
            let mut nevents: u32 = 0;
            for i in 0..nsubs as usize {
                let sub_offset = in_ptr as usize + i * 48;
                let mut sub = [0u8; 48];
                if mem.read(&caller, sub_offset, &mut sub).is_err() {
                    continue;
                }
                let userdata = u64::from_le_bytes(sub[0..8].try_into().unwrap_or([0u8; 8]));
                let tag = sub[8];
                let mut ev = [0u8; 32];
                ev[0..8].copy_from_slice(&userdata.to_le_bytes());
                match tag {
                    0 => {
                        // CLOCK — treat every deadline as already elapsed so goroutines
                        // that sleep via time.Sleep or runtime timers can proceed.
                        ev[10] = 0; // type = CLOCK
                        out_buf.extend_from_slice(&ev);
                        nevents += 1;
                    }
                    1 => {
                        // FD_READ
                        let fd = u32::from_le_bytes(sub[16..20].try_into().unwrap_or([0u8; 4]));
                        if fd == 0 {
                            if stdin_has_data {
                                let nb = (caller.data().stdin_buf.len()
                                    - caller.data().stdin_pos) as u64;
                                ev[10] = 1; // type = FD_READ
                                ev[16..24].copy_from_slice(&nb.to_le_bytes());
                                out_buf.extend_from_slice(&ev);
                                nevents += 1;
                            }
                            // If stdin is empty (EOF): don't report as ready.
                            // fd_read will return nread=0 → Go interprets as EOF.
                        }
                        // Other fds: not supported, skip (no event reported).
                    }
                    2 => {
                        // FD_WRITE — stdout (1) and stderr (2) are always writable.
                        let fd = u32::from_le_bytes(sub[16..20].try_into().unwrap_or([0u8; 4]));
                        if fd == 1 || fd == 2 {
                            ev[10] = 2; // type = FD_WRITE
                            ev[16..24].copy_from_slice(&65536u64.to_le_bytes());
                            out_buf.extend_from_slice(&ev);
                            nevents += 1;
                        }
                    }
                    _ => {}
                }
            }
            // Fallback: if no subscriptions produced events (e.g. the goroutine
            // scheduler is waiting solely on a stdin FD_READ after EOF, or on an
            // unknown subscription type), synthesise a CLOCK event so the scheduler
            // yields rather than spinning.  This prevents infinite busy-wait.
            if nevents == 0 {
                let mut ev = [0u8; 32];
                ev[10] = 0; // CLOCK
                out_buf.extend_from_slice(&ev);
                nevents = 1;
            }
            let _ = mem.write(&mut caller, out_ptr as usize, &out_buf);
            let _ = mem.write(&mut caller, nevents_ptr as usize, &nevents.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_close(fd: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "fd_close",
        |_: Caller<HostState>, _fd: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_fdstat_get(fd: i32, stat_out: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_get",
        |mut caller: Caller<HostState>, fd: i32, stat_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            // fdstat struct layout (24 bytes, little-endian):
            //   offset 0:  filetype  (u8)  — 2 = FILETYPE_CHARACTER_DEVICE (stdin/stdout/stderr)
            //   offset 1:  padding   (u8)
            //   offset 2:  fs_flags  (u16) — 0 (no APPEND, no NONBLOCK)
            //   offset 4:  padding   (u32)
            //   offset 8:  rights_base  (u64) — all rights granted so Ruby/Python don't skip fd_write
            //   offset 16: rights_inheriting (u64) — all rights
            //
            // Rights must be non-zero for stdin/stdout/stderr or runtimes like Ruby WASM will
            // refuse to write to fd=1 (they check rights before calling fd_write).
            let mut stat = [0u8; 24];
            // fd 0/1/2 are character devices; all others return EBADF
            if fd < 0 || fd > 2 {
                return 8; // EBADF
            }
            stat[0] = 2; // FILETYPE_CHARACTER_DEVICE
            let all_rights: u64 = u64::MAX;
            stat[8..16].copy_from_slice(&all_rights.to_le_bytes());
            stat[16..24].copy_from_slice(&all_rights.to_le_bytes());
            let _ = mem.write(&mut caller, stat_out as usize, &stat);
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_fdstat_set_flags(fd: i32, flags: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "fd_fdstat_set_flags",
        |_: Caller<HostState>, _fd: i32, _flags: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_prestat_get(fd: i32, prestat_out: i32) -> i32
    // Return EBADF (8) for all fds — no pre-opened dirs.
    linker.func_wrap("wasi_snapshot_preview1", "fd_prestat_get",
        |_: Caller<HostState>, _fd: i32, _prestat_out: i32| -> i32 { 8 }
    ).map_err(|e| e.to_string())?;

    // fd_prestat_dir_name(fd: i32, path: i32, path_len: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "fd_prestat_dir_name",
        |_: Caller<HostState>, _fd: i32, _path: i32, _path_len: i32| -> i32 { 8 }
    ).map_err(|e| e.to_string())?;

    // fd_advise(fd: i32, offset: i64, len: i64, advice: i32) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "fd_advise",
        |_: Caller<HostState>, _fd: i32, _offset: i64, _len: i64, _advice: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_datasync(fd: i32) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "fd_datasync",
        |_: Caller<HostState>, _fd: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_sync(fd: i32) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "fd_sync",
        |_: Caller<HostState>, _fd: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_tell(fd: i32, offset_out: i32) -> i32
    // Returns ESPIPE (29) for stdin/stdout; 0 with offset=0 otherwise.
    linker.func_wrap("wasi_snapshot_preview1", "fd_tell",
        |mut caller: Caller<HostState>, fd: i32, offset_out: i32| -> i32 {
            if fd == 0 || fd == 1 || fd == 2 { return 29; } // ESPIPE
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, offset_out as usize, &0u64.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_seek(fd: i32, offset: i64, whence: i32, new_offset_out: i32) -> i32
    // Returns ESPIPE for stdin/stdout; EBADF for other fds.
    linker.func_wrap("wasi_snapshot_preview1", "fd_seek",
        |mut caller: Caller<HostState>, fd: i32, _offset: i64, _whence: i32, new_offset_out: i32| -> i32 {
            if fd == 0 || fd == 1 || fd == 2 { return 29; } // ESPIPE
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, new_offset_out as usize, &0u64.to_le_bytes());
            8 // EBADF for anything else
        }
    ).map_err(|e| e.to_string())?;

    // fd_pread(fd: i32, iovs: i32, iovs_len: i32, offset: i64, nread: i32) -> i32
    // Returns EBADF — no seekable reads in this runtime.
    linker.func_wrap("wasi_snapshot_preview1", "fd_pread",
        |mut caller: Caller<HostState>, _fd: i32, _iovs: i32, _iovs_len: i32, _offset: i64, nread: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, nread as usize, &0u32.to_le_bytes());
            0 // return 0 bytes read (EOF)
        }
    ).map_err(|e| e.to_string())?;

    // fd_pwrite(fd: i32, iovs: i32, iovs_len: i32, offset: i64, nwritten: i32) -> i32
    // Discard writes (no seekable write support).
    linker.func_wrap("wasi_snapshot_preview1", "fd_pwrite",
        |mut caller: Caller<HostState>, _fd: i32, _iovs: i32, _iovs_len: i32, _offset: i64, nwritten: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, nwritten as usize, &0u32.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_filestat_get(fd: i32, stat_out: i32) -> i32
    // Return zero-filled stat struct (64 bytes).
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_get",
        |mut caller: Caller<HostState>, _fd: i32, stat_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, stat_out as usize, &[0u8; 64]);
            0
        }
    ).map_err(|e| e.to_string())?;

    // fd_filestat_set_size(fd: i32, size: i64) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_set_size",
        |_: Caller<HostState>, _fd: i32, _size: i64| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_filestat_set_times(fd: i32, atim: i64, mtim: i64, fst_flags: i32) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "fd_filestat_set_times",
        |_: Caller<HostState>, _fd: i32, _atim: i64, _mtim: i64, _fst_flags: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // fd_readdir(fd: i32, buf: i32, buf_len: i32, cookie: i64, bufused_out: i32) -> i32
    // Return empty directory listing (no entries).
    linker.func_wrap("wasi_snapshot_preview1", "fd_readdir",
        |mut caller: Caller<HostState>, _fd: i32, _buf: i32, _buf_len: i32, _cookie: i64, bufused_out: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, bufused_out as usize, &0u32.to_le_bytes());
            0
        }
    ).map_err(|e| e.to_string())?;

    // path_open(dirfd: i32, dirflags: i32, path: i32, path_len: i32,
    //           oflags: i32, fs_rights_base: i64, fs_rights_inheriting: i64,
    //           fdflags: i32, fd_out: i32) -> i32 — ENOENT (always)
    linker.func_wrap("wasi_snapshot_preview1", "path_open",
        |_: Caller<HostState>, _dirfd: i32, _dirflags: i32, _path: i32, _path_len: i32,
         _oflags: i32, _rights_base: i64, _rights_inh: i64, _fdflags: i32, _fd_out: i32| -> i32 {
            44 // ENOENT
        }
    ).map_err(|e| e.to_string())?;

    // path_filestat_get(dirfd: i32, flags: i32, path: i32, path_len: i32, stat_out: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "path_filestat_get",
        |_: Caller<HostState>, _dirfd: i32, _flags: i32, _path: i32, _path_len: i32, _stat_out: i32| -> i32 {
            44 // ENOENT
        }
    ).map_err(|e| e.to_string())?;

    // path_filestat_set_times(dirfd: i32, flags: i32, path: i32, path_len: i32,
    //                         atim: i64, mtim: i64, fst_flags: i32) -> i32 — no-op
    linker.func_wrap("wasi_snapshot_preview1", "path_filestat_set_times",
        |_: Caller<HostState>, _dirfd: i32, _flags: i32, _path: i32, _path_len: i32,
         _atim: i64, _mtim: i64, _fst_flags: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // path_create_directory(dirfd: i32, path: i32, path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_create_directory",
        |_: Caller<HostState>, _dirfd: i32, _path: i32, _path_len: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_remove_directory(dirfd: i32, path: i32, path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_remove_directory",
        |_: Caller<HostState>, _dirfd: i32, _path: i32, _path_len: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_rename(old_fd: i32, old_path: i32, old_path_len: i32,
    //             new_fd: i32, new_path: i32, new_path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_rename",
        |_: Caller<HostState>, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_link(old_fd: i32, old_flags: i32, old_path: i32, old_path_len: i32,
    //           new_fd: i32, new_path: i32, new_path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_link",
        |_: Caller<HostState>, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_symlink(old_path: i32, old_path_len: i32, fd: i32,
    //              new_path: i32, new_path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_symlink",
        |_: Caller<HostState>, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_unlink_file(fd: i32, path: i32, path_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "path_unlink_file",
        |_: Caller<HostState>, _: i32, _: i32, _: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // path_readlink(fd: i32, path: i32, path_len: i32, buf: i32, buf_len: i32, nread: i32) -> i32
    linker.func_wrap("wasi_snapshot_preview1", "path_readlink",
        |mut caller: Caller<HostState>, _fd: i32, _path: i32, _path_len: i32,
         _buf: i32, _buf_len: i32, nread: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 8,
            };
            let _ = mem.write(&mut caller, nread as usize, &0u32.to_le_bytes());
            44 // ENOENT
        }
    ).map_err(|e| e.to_string())?;

    // sock_open(family: i32, socktype: i32, protocol: i32) -> i32 — ENOSYS
    // PHP uses this for networking; we don't support WASI sockets.
    linker.func_wrap("wasi_snapshot_preview1", "sock_open",
        |_: Caller<HostState>, _family: i32, _socktype: i32, _protocol: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_bind(fd: i32, addr: i32, port: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_bind",
        |_: Caller<HostState>, _fd: i32, _addr: i32, _port: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_listen(fd: i32, backlog: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_listen",
        |_: Caller<HostState>, _fd: i32, _backlog: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_setsockopt(fd: i32, level: i32, name: i32, buf: i32, buf_len: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_setsockopt",
        |_: Caller<HostState>, _fd: i32, _level: i32, _name: i32, _buf: i32, _buf_len: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_connect(fd: i32, addr: i32, port: i32) -> i32 — ENOSYS
    // PHP imports this for socket networking; we deny all outbound socket connections.
    linker.func_wrap("wasi_snapshot_preview1", "sock_connect",
        |_: Caller<HostState>, _fd: i32, _addr: i32, _port: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_accept — signature varies by WASM runtime:
    //   PHP (WasmEdge): (fd: i32, addr_out: i32) -> i32           [2 params]
    //   Python / standard WASI: (fd: i32, flags: i32, result_fd: i32) -> i32  [3 params]
    // Detect which variant this module imports and register the right ENOSYS stub.
    if let Some(import) = module.imports()
        .find(|i| i.module() == "wasi_snapshot_preview1" && i.name() == "sock_accept")
    {
        if let wasmtime::ExternType::Func(ft) = import.ty() {
            linker.func_new("wasi_snapshot_preview1", "sock_accept", ft,
                |_caller, _params, results| {
                    results[0] = Val::I32(52); // ENOSYS
                    Ok(())
                }
            ).map_err(|e| e.to_string())?;
        }
    }

    // sock_recv(fd: i32, ri_data: i32, ri_data_len: i32, ri_flags: i32, ro_datalen: i32, ro_flags: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_recv",
        |_: Caller<HostState>, _fd: i32, _ri_data: i32, _ri_data_len: i32, _ri_flags: i32, _ro_datalen: i32, _ro_flags: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_send(fd: i32, si_data: i32, si_data_len: i32, si_flags: i32, so_datalen: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_send",
        |_: Caller<HostState>, _fd: i32, _si_data: i32, _si_data_len: i32, _si_flags: i32, _so_datalen: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // sock_shutdown(fd: i32, how: i32) -> i32 — ENOSYS
    linker.func_wrap("wasi_snapshot_preview1", "sock_shutdown",
        |_: Caller<HostState>, _fd: i32, _how: i32| -> i32 { 52 }
    ).map_err(|e| e.to_string())?;

    // fd_renumber(from: i32, to: i32) -> i32 — used by Ruby; no-op since we have no real fd table
    linker.func_wrap("wasi_snapshot_preview1", "fd_renumber",
        |_: Caller<HostState>, _from: i32, _to: i32| -> i32 { 0 }
    ).map_err(|e| e.to_string())?;

    // ── Flux host imports ────────────────────────────────────────────────────

    // flux.log(level: i32, msg_ptr: i32, msg_len: i32)
    linker.func_wrap("flux", "log", |mut caller: Caller<HostState>, level: i32, msg_ptr: i32, msg_len: i32| {
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

    // flux.http_fetch(req_ptr, req_len, out_ptr, out_max) → actual_resp_len or -1
    //
    // Uses func_wrap_async so the WASM fiber is suspended during the HTTP call,
    // freeing the tokio thread to drive other concurrent WASM executions.
    // Previously used block_on() which blocked the OS thread for the full HTTP duration.
    linker.func_wrap_async("flux", "http_fetch", |mut caller: Caller<HostState>, args: (i32, i32, i32, i32)| {
        let (req_ptr, req_len, out_ptr, out_max) = args;
        Box::new(async move {
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None => return Ok::<i32, wasmtime::Error>(-1),
        };

        // Read request JSON from WASM memory.
        let req_json = {
            let data = memory.data(&caller);
            let end  = (req_ptr as usize).saturating_add(req_len as usize);
            if end > data.len() { return Ok(-1); }
            match serde_json::from_slice::<serde_json::Value>(&data[req_ptr as usize..end]) {
                Ok(v)  => v,
                Err(_) => return Ok(-1),
            }
        };

        let url = match req_json.get("url").and_then(|u| u.as_str()) {
            Some(u) => u.to_string(),
            None    => return Ok(-1),
        };

        // ── Allow-list check ─────────────────────────────────────────────────
        let allowed = {
            let hosts = &caller.data().allowed_http_hosts;
            let parsed = match url.parse::<reqwest::Url>() {
                Ok(p)  => p,
                Err(_) => {
                    caller.data_mut().logs.push(LogLine {
                        level:           "warn".to_string(),
                        message:         format!("http_fetch blocked: invalid URL: {}", url),
                        span_type:       None,
                        source:          Some("function".to_string()),
                        span_id:         None,
                        duration_ms:     None,
                        execution_state: None,
                        tool_name:       None,
                    });
                    return Ok(-1);
                }
            };

            if is_private_host(&parsed) {
                caller.data_mut().logs.push(LogLine {
                    level:           "warn".to_string(),
                    message:         format!("http_fetch blocked: private/internal address not permitted: {}", url),
                    span_type:       None,
                    source:          Some("function".to_string()),
                    span_id:         None,
                    duration_ms:     None,
                    execution_state: None,
                    tool_name:       None,
                });
                return Ok(-1);
            }

            if hosts.iter().any(|h| h == "*") {
                true
            } else {
                let host_str = parsed.host_str().unwrap_or("");
                hosts.iter().any(|h| h == host_str)
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
            return Ok(-1);
        }

        let method_str = req_json.get("method").and_then(|m| m.as_str()).unwrap_or("GET").to_uppercase();
        let body_b64   = req_json.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string();
        let headers    = req_json.get("headers").and_then(|h| h.as_object()).cloned();

        // ── Make the async HTTP request — fiber suspends here ─────────────────
        let client = caller.data().http_client.clone();
        let resp_json = {
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
        };

        // ── Write response JSON to WASM memory ────────────────────────────────
        let resp_bytes = match serde_json::to_vec(&resp_json) {
            Ok(b)  => b,
            Err(_) => return Ok(-1),
        };
        if resp_bytes.len() > out_max as usize { return Ok(-1); }

        let data = memory.data_mut(&mut caller);
        let out_start = out_ptr as usize;
        let out_end   = out_start + resp_bytes.len();
        if out_end > data.len() { return Ok(-1); }
        data[out_start..out_end].copy_from_slice(&resp_bytes);
        Ok(resp_bytes.len() as i32)
        })
    }).map_err(|e| e.to_string())?;

    // flux.secrets_get(key_ptr, key_len, out_ptr, out_max) → actual_len or -1
    linker.func_wrap("flux", "secrets_get", |mut caller: Caller<HostState>, key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32| -> i32 {
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

    // ── Instantiate (async with async_support engine) ──────────────────────

    // flux.db_query(sql_ptr: i32, sql_len: i32, params_ptr: i32, params_len: i32,
    //                   out_ptr: i32, out_max: i32) → i32 (bytes written, or -1 on error)
    // Sends a raw SQL request to the data-engine /db/sql endpoint and writes the
    // JSON result (array of rows or {rows_affected: N}) into WASM linear memory.
    linker.func_wrap_async("flux", "db_query", |mut caller: Caller<HostState>, args: (i32, i32, i32, i32, i32, i32)| {
        let (sql_ptr, sql_len, params_ptr, params_len, out_ptr, out_max) = args;
        Box::new(async move {
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1i32,
            };
            let data = memory.data(&caller);

            // Read SQL string
            let sql_end = (sql_ptr as usize).saturating_add(sql_len as usize);
            if sql_end > data.len() { return -1; }
            let sql = match std::str::from_utf8(&data[sql_ptr as usize..sql_end]) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };

            // Read optional JSON params array (may be zero-length → null)
            let sql_params: serde_json::Value = if params_len > 0 {
                let params_end = (params_ptr as usize).saturating_add(params_len as usize);
                if params_end > data.len() { return -1; }
                match serde_json::from_slice(&data[params_ptr as usize..params_end]) {
                    Ok(v) => v,
                    Err(_) => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::Null
            };

            let database = caller.data().database.clone();
            let de = caller.data().dispatchers.data_engine.clone();

            let params_vec = match sql_params {
                serde_json::Value::Array(arr) => arr,
                serde_json::Value::Null => vec![],
                other => vec![other],
            };

            let response_bytes = match de.execute_sql(sql, params_vec, database, String::new()).await {
                Ok(val) => serde_json::to_vec(&val).unwrap_or_default(),
                Err(e) => {
                    let err_json = format!("{{\"error\":\"{}\"}}", e.replace('"', "\\\""));
                    err_json.into_bytes()
                }
            };

            let write_len = response_bytes.len().min(out_max as usize);
            let data = memory.data_mut(&mut caller);
            let out_start = out_ptr as usize;
            let out_end   = out_start + write_len;
            if out_end > data.len() { return -1; }
            data[out_start..out_end].copy_from_slice(&response_bytes[..write_len]);
            write_len as i32
        })
    }).map_err(|e| e.to_string())?;

    // flux.queue_push(req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32) → i32
    // Enqueues a job via the Flux queue service. `req_ptr` points to a JSON object
    // `{function: string, payload: any, delay_secs?: number}`.
    linker.func_wrap_async("flux", "queue_push", |mut caller: Caller<HostState>, args: (i32, i32, i32, i32)| {
        let (req_ptr, req_len, out_ptr, out_max) = args;
        Box::new(async move {
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1i32,
            };
            let data = memory.data(&caller);

            let req_end = (req_ptr as usize).saturating_add(req_len as usize);
            if req_end > data.len() { return -1; }
            let req: serde_json::Value = match serde_json::from_slice(&data[req_ptr as usize..req_end]) {
                Ok(v) => v,
                Err(_) => return -1,
            };

            let api = caller.data().dispatchers.api.clone();
            let queue = caller.data().dispatchers.queue.clone();

            let function_name = match req.get("function").and_then(|f| f.as_str()) {
                Some(name) => name.to_string(),
                None => return -1,
            };
            let payload = req.get("payload").cloned().unwrap_or(serde_json::Value::Null);
            let delay_secs = req.get("delay_secs").and_then(|d| d.as_u64());

            // Resolve function name → ID
            let resolved = match api.resolve_function(&function_name).await {
                Ok(r) => r,
                Err(e) => {
                    let err_json = format!("{{\"error\":\"resolve failed: {}\"}}", e.replace('"', "\\\""));
                    let err_bytes = err_json.as_bytes();
                    let write_len = err_bytes.len().min(out_max as usize);
                    let data = memory.data_mut(&mut caller);
                    let out_start = out_ptr as usize;
                    if out_start + write_len <= data.len() {
                        data[out_start..out_start + write_len].copy_from_slice(&err_bytes[..write_len]);
                    }
                    return write_len as i32;
                }
            };

            // Push job via dispatch
            let response_bytes = match queue.push_job(
                &resolved.function_id.to_string(),
                payload,
                delay_secs,
                None,
            ).await {
                Ok(()) => b"{\"ok\":true}".to_vec(),
                Err(e) => {
                    let err_json = format!("{{\"error\":\"queue_push failed: {}\"}}", e.replace('"', "\\\""));
                    err_json.into_bytes()
                }
            };

            let write_len = response_bytes.len().min(out_max as usize);
            let data = memory.data_mut(&mut caller);
            let out_start = out_ptr as usize;
            let out_end   = out_start + write_len;
            if out_end > data.len() { return -1; }
            data[out_start..out_end].copy_from_slice(&response_bytes[..write_len]);
            write_len as i32
        })
    }).map_err(|e| e.to_string())?;

    // ── Instantiate (async with async_support engine) ──────────────────────

    let instance = linker.instantiate_async(&mut store, module).await
        .map_err(|e| format!("wasm instantiation failed: {}", e))?;

    // ── Detect execution model ─────────────────────────────────────────────
    //
    // Two models are supported:
    //
    //  • Custom Flux ABI: exports `__flux_alloc` + `handle`.
    //    Used by Rust (wasm32-unknown-unknown) and AssemblyScript.
    //    Input is passed via direct memory write; result is read back from memory.
    //
    //  • WASI stdin/stdout: exports `_start` (command model).
    //    Used by Go (GOOS=wasip1), C/wasi-sdk, and other WASI-compatible runtimes.
    //    Input JSON is served on fd=0 (stdin); result JSON is captured from fd=1 (stdout).
    //    `proc_exit(0)` traps with a recognizable sentinel to stop execution cleanly.
    //
    //  • WASI reactor: exports `_initialize` (reactor model) + `handle` or similar.
    //    Call `_initialize` first, then dispatch via custom ABI.

    let has_flux_alloc = instance.get_export(&mut store, "__flux_alloc").is_some();
    let has_handle     = instance.get_export(&mut store, "handle").is_some();
    let has_initialize = instance.get_export(&mut store, "_initialize").is_some();
    let has_start      = instance.get_export(&mut store, "_start").is_some();

    if has_initialize {
        // WASI reactor bootstrap
        let init_fn = instance.get_typed_func::<(), ()>(&mut store, "_initialize")
            .map_err(|e| format!("wasm _initialize lookup failed: {}", e))?;
        init_fn.call_async(&mut store, ()).await
            .map_err(|e| format!("wasm _initialize failed: {}", e))?;
    }

    if has_flux_alloc && has_handle {
        // ── Custom Flux ABI path ───────────────────────────────────────────

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or("wasm module missing required 'memory' export")?;

        let alloc_fn = instance
            .get_typed_func::<i32, i32>(&mut store, "__flux_alloc")
            .map_err(|_| "wasm module missing required '__flux_alloc' export")?;

        let handle_fn = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "handle")
            .map_err(|_| "wasm module missing required 'handle' export")?;

        let payload_json = serde_json::to_string(&params.payload)
            .map_err(|e| format!("payload serialization failed: {}", e))?;
        let payload_bytes = payload_json.as_bytes();
        let payload_len   = payload_bytes.len() as i32;

        let payload_ptr = alloc_fn.call_async(&mut store, payload_len).await
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

        let result_ptr = handle_fn
            .call_async(&mut store, (payload_ptr, payload_len)).await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("fuel") || msg.contains("trap: out of fuel") {
                    "wasm function exceeded CPU fuel limit".to_string()
                } else {
                    format!("wasm handle trap: {}", msg)
                }
            })?;

        // Layout: [4 bytes u32 LE = length][<length> bytes UTF-8 JSON]
        let result_json = {
            let data = memory.data(&store);
            if result_ptr <= 0 {
                return Err("wasm handle returned null result pointer".to_string());
            }
            let ptr     = result_ptr as usize;
            let hdr_end = ptr + 4;
            if hdr_end > data.len() {
                return Err("result pointer out of bounds reading length header".to_string());
            }
            let result_len = u32::from_le_bytes([data[ptr], data[ptr+1], data[ptr+2], data[ptr+3]]) as usize;
            let json_end   = hdr_end + result_len;
            if json_end > data.len() {
                return Err(format!("result length {} overflows linear memory at offset {}", result_len, hdr_end));
            }
            match std::str::from_utf8(&data[hdr_end..json_end]) {
                Ok(s)  => s.to_string(),
                Err(e) => return Err(format!("result JSON is not valid UTF-8: {}", e)),
            }
        };

        let result_value: serde_json::Value = serde_json::from_str(&result_json)
            .map_err(|e| format!("result JSON parse error: {} — raw: {:.256}", e, result_json))?;

        if let Some(err_msg) = result_value.get("error").and_then(|v| v.as_str()) {
            return Err(serde_json::json!({
                "code":    "FunctionExecutionError",
                "message": err_msg
            }).to_string());
        }

        let output = result_value.get("output").cloned().unwrap_or(result_value);
        let logs = store.into_data().logs;
        return Ok(ExecutionResult { output, logs });
    }

    if has_start {
        // ── WASI stdin/stdout path ─────────────────────────────────────────
        //
        // The module reads its input from stdin (fd=0) and writes its JSON
        // result to stdout (fd=1), then calls proc_exit(0).
        // Our proc_exit stub traps with "__wasi_proc_exit:0" to stop _start cleanly.

        let payload_json = serde_json::to_string(&params.payload)
            .map_err(|e| format!("payload serialization failed: {}", e))?;
        store.data_mut().stdin_buf = payload_json.into_bytes();
        store.data_mut().stdin_pos = 0;

        let start_fn = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|e| format!("wasm _start lookup failed: {}", e))?;

        let start_result = start_fn.call_async(&mut store, ()).await;
        match start_result {
            Ok(()) => {} // _start returned normally without proc_exit
            Err(e) => {
                // Use {:#} to include the full anyhow error chain (incl. "Caused by:" sections).
                // Our proc_exit stub emits `anyhow::bail!("__wasi_proc_exit:N")` as the root
                // cause; the outer trap message added by wasmtime contains the WASM backtrace.
                let full_msg = format!("{:#}", e);
                let short_msg = e.to_string();
                if full_msg.contains("fuel") || full_msg.contains("trap: out of fuel") {
                    return Err("wasm function exceeded CPU fuel limit".to_string());
                } else if full_msg.contains("__wasi_proc_exit:") {
                    // Normal or error exit via proc_exit.  Non-zero exit is a crash.
                    if !full_msg.contains("__wasi_proc_exit:0") {
                        // Go (and C) write the panic/error to stderr before calling proc_exit.
                        let stderr = String::from_utf8_lossy(
                            &store.data().stderr_buf
                        ).into_owned();
                        let reason = if stderr.trim().is_empty() {
                            "non-zero exit without stderr output".to_string()
                        } else {
                            stderr.trim().to_string()
                        };
                        return Err(format!("wasm function exited with error: {}", reason));
                    }
                    // proc_exit(0) — success
                } else {
                    // Genuine WASM trap (nil deref, unreachable, OOM, etc.)
                    let stderr = String::from_utf8_lossy(
                        &store.data().stderr_buf
                    ).into_owned();
                    if stderr.trim().is_empty() {
                        return Err(format!("wasm _start trap: {}", short_msg));
                    }
                    return Err(format!("wasm _start trap: {} — stderr: {}", short_msg, stderr.trim()));
                }
            }
        }

        let stdout_bytes = std::mem::take(&mut store.data_mut().stdout_buf);
        if stdout_bytes.is_empty() {
            let stderr = String::from_utf8_lossy(&store.data().stderr_buf).into_owned();
            if stderr.trim().is_empty() {
                return Err("wasm function produced no output on stdout".to_string());
            }
            return Err(format!("wasm function produced no stdout; stderr: {}", stderr.trim()));
        }

        let stdout_str = std::str::from_utf8(&stdout_bytes)
            .map_err(|e| format!("wasm stdout is not valid UTF-8: {}", e))?;
        // Trim trailing newline (Go's json.Encoder adds one)
        let stdout_str = stdout_str.trim();

        let result_value: serde_json::Value = serde_json::from_str(stdout_str)
            .map_err(|e| format!("wasm stdout JSON parse error: {} — raw: {:.256}", e, stdout_str))?;

        if let Some(err_val) = result_value.get("error") {
            let code = result_value.get("code").and_then(|v| v.as_str()).unwrap_or("FunctionExecutionError");
            let msg  = err_val.as_str().unwrap_or("unknown error");
            return Err(serde_json::json!({
                "code":    code,
                "message": msg
            }).to_string());
        }

        // stdout IS the output (no "output" wrapper for stdin/stdout model)
        let output = result_value.get("output").cloned().unwrap_or(result_value);
        let logs = store.into_data().logs;
        return Ok(ExecutionResult { output, logs });
    }

    Err("wasm module must export either (__flux_alloc + handle) or _start".to_string())
}

/// Returns `true` if the URL's host resolves to a private, loopback, or
/// link-local address that must never be reachable from user functions.
///
/// Blocked ranges:
/// - 127.x.x.x          loopback
/// - 10.x.x.x           RFC 1918 private
/// - 172.16-31.x.x      RFC 1918 private
/// - 192.168.x.x        RFC 1918 private
/// - 169.254.x.x        link-local / cloud metadata (AWS/GCP/Azure IMDS)
/// - ::1                 IPv6 loopback
/// - fc00::/7           IPv6 unique-local
fn is_private_host(url: &reqwest::Url) -> bool {
    use std::net::IpAddr;

    let host = match url.host() {
        Some(h) => h,
        None    => return true, // no host = block
    };

    match host {
        url::Host::Ipv4(ip) => {
            let o = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || o[0] == 169 && o[1] == 254   // cloud metadata (link-local)
                || ip.is_unspecified()
        }
        url::Host::Ipv6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || {
                    let segs = ip.segments();
                    // fc00::/7  (unique-local)
                    (segs[0] & 0xfe00) == 0xfc00
                    // fe80::/10 (link-local)
                    || (segs[0] & 0xffc0) == 0xfe80
                }
        }
        url::Host::Domain(d) => {
            let d = d.to_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || d.ends_with(".corp")
                // Try parsing as IP (e.g. "127.0.0.1" given as domain)
                || d.parse::<IpAddr>().map(|ip| match ip {
                    IpAddr::V4(v4) => {
                        let o = v4.octets();
                        v4.is_loopback() || v4.is_private() || v4.is_link_local()
                            || (o[0] == 169 && o[1] == 254)
                    }
                    IpAddr::V6(v6) => {
                        let segs = v6.segments();
                        v6.is_loopback()
                            || (segs[0] & 0xfe00) == 0xfc00
                            || (segs[0] & 0xffc0) == 0xfe80
                    }
                }).unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock};
    use async_trait::async_trait;
    use job_contract::dispatch::{
        ApiDispatch, DataEngineDispatch, QueueDispatch, RuntimeDispatch,
        ExecuteRequest, ExecuteResponse, ResolvedFunction,
    };

    struct MockApiDispatch;
    #[async_trait]
    impl ApiDispatch for MockApiDispatch {
        async fn get_bundle(&self, _: &str) -> Result<serde_json::Value, String> { Err("mock".into()) }
        async fn write_log(&self, _: serde_json::Value) -> Result<(), String> { Ok(()) }
        async fn write_network_call(&self, _: serde_json::Value) -> Result<(), String> { Ok(()) }
        async fn get_secrets(&self) -> Result<HashMap<String, String>, String> { Ok(HashMap::new()) }
        async fn resolve_function(&self, _: &str) -> Result<ResolvedFunction, String> { Err("mock".into()) }
    }

    struct MockQueueDispatch;
    #[async_trait]
    impl QueueDispatch for MockQueueDispatch {
        async fn push_job(&self, _: &str, _: serde_json::Value, _: Option<u64>, _: Option<String>) -> Result<(), String> { Err("mock".into()) }
    }

    struct MockDataEngineDispatch;
    #[async_trait]
    impl DataEngineDispatch for MockDataEngineDispatch {
        async fn execute_sql(&self, _: String, _: Vec<serde_json::Value>, _: String, _: String) -> Result<serde_json::Value, String> { Err("mock".into()) }
    }

    fn mock_dispatchers() -> PoolDispatchers {
        PoolDispatchers {
            api:         Arc::new(MockApiDispatch),
            queue:       Arc::new(MockQueueDispatch),
            data_engine: Arc::new(MockDataEngineDispatch),
            runtime:     Arc::new(OnceLock::new()),
        }
    }

    /// Minimal WAT module satisfying the flux WASM ABI.
    ///
    /// Memory layout at result pointer 4:
    ///   bytes 4-7  : u32 LE = 15  (length of JSON below)
    ///   bytes 8-22 : `{"output":"ok"}`
    ///
    /// handle() returns 4 (non-zero) so the executor can read the header.
    /// __flux_alloc() returns 65536 (page boundary) as the payload write buffer.
    const MINIMAL_WAT: &str = r#"(module
        (import "flux" "log"         (func (param i32 i32 i32)))
        (import "flux" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "flux" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 4) "\0f\00\00\00{\"output\":\"ok\"}")
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
        (func (export "handle") (param i32 i32) (result i32) i32.const 4)
    )"#;

    /// WAT module that returns an error in the result JSON.
    const ERROR_WAT: &str = r#"(module
        (import "flux" "log"         (func (param i32 i32 i32)))
        (import "flux" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "flux" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 4) "\16\00\00\00{\"error\":\"test_error\"}")
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
        (func (export "handle") (param i32 i32) (result i32) i32.const 4)
    )"#;

    /// WAT module missing the `handle` export — should fail instantiation.
    const MISSING_HANDLE_WAT: &str = r#"(module
        (import "flux" "log"         (func (param i32 i32 i32)))
        (import "flux" "secrets_get" (func (param i32 i32 i32 i32) (result i32)))
        (import "flux" "http_fetch"  (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (func (export "__flux_alloc") (param i32) (result i32) i32.const 65536)
    )"#;

    fn wasm_bytes(wat: &str) -> Vec<u8> {
        wat::parse_str(wat).expect("WAT compilation failed")
    }

    fn default_params() -> WasmExecutionParams {
        let mut p = WasmExecutionParams::test_default(mock_dispatchers());
        p.payload = serde_json::json!({"msg": "test"});
        p
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

        let mut params = WasmExecutionParams::test_default(mock_dispatchers());
        params.secrets = secrets;
        params.payload = serde_json::json!({});
        // The minimal module doesn't call secrets_get, but execution should succeed.
        let result = execute_wasm(&engine, &module, params).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn execute_wasm_default_fuel_limit_is_nonzero() {
        let params = WasmExecutionParams::test_default(mock_dispatchers());
        assert!(params.fuel_limit > 0);
    }

    // ── WasmExecutionParams default ───────────────────────────────────────

    #[test]
    fn wasm_params_default_payload_is_null() {
        let p = WasmExecutionParams::test_default(mock_dispatchers());
        assert!(p.payload.is_null());
    }

    #[test]
    fn wasm_params_default_allowed_hosts_is_empty() {
        let p = WasmExecutionParams::test_default(mock_dispatchers());
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

    // ── SSRF private-IP blocking tests ────────────────────────────────────────

    fn parse(url: &str) -> reqwest::Url { url.parse().unwrap() }

    #[test]
    fn ssrf_loopback_ipv4_blocked() {
        assert!(is_private_host(&parse("http://127.0.0.1/secret")));
    }

    #[test]
    fn ssrf_localhost_domain_blocked() {
        assert!(is_private_host(&parse("http://localhost:8080/secret")));
    }

    #[test]
    fn ssrf_rfc1918_10_blocked() {
        assert!(is_private_host(&parse("http://10.0.0.1/secret")));
    }

    #[test]
    fn ssrf_rfc1918_172_16_blocked() {
        assert!(is_private_host(&parse("http://172.16.0.1/secret")));
    }

    #[test]
    fn ssrf_rfc1918_192_168_blocked() {
        assert!(is_private_host(&parse("http://192.168.1.1/secret")));
    }

    #[test]
    fn ssrf_cloud_metadata_imds_blocked() {
        assert!(is_private_host(&parse("http://169.254.169.254/latest/meta-data/")));
    }

    #[test]
    fn ssrf_dot_local_domain_blocked() {
        assert!(is_private_host(&parse("http://postgres.local/query")));
    }

    #[test]
    fn ssrf_dot_internal_domain_blocked() {
        assert!(is_private_host(&parse("http://api.internal/admin")));
    }

    #[test]
    fn ssrf_ipv6_loopback_blocked() {
        assert!(is_private_host(&parse("http://[::1]/secret")));
    }

    #[test]
    fn ssrf_public_ip_allowed() {
        assert!(!is_private_host(&parse("https://8.8.8.8/dns")));
    }

    #[test]
    fn ssrf_public_domain_allowed() {
        assert!(!is_private_host(&parse("https://api.example.com/v1/data")));
    }
}
