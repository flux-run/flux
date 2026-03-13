//! `runtime` library crate — sandboxed user-code execution.
//!
//! ## Mental model
//!
//! Runtime executes user code in sandboxed **V8** (Deno) or **WASM** (Wasmtime) isolates.
//! It **never touches Postgres directly**. All state access goes through the `ctx` object
//! which proxies to other services:
//!
//! - `ctx.db.*`       → POST data-engine `/db/query`
//! - `ctx.queue.*`    → POST queue service `/jobs`
//! - `ctx.secrets.*`  → `ApiDispatch::get_secrets` (with LRU cache)
//! - `ctx.log()`      → `ApiDispatch::write_log` → `flux.platform_logs` (fire-and-forget)
//! - `ctx.agent.*`    → `AgentDispatch::run`
//!
//! ## Execution paths
//!
//! ```text
//! POST /execute (HTTP)
//!        ↓
//! execute_handler
//!  ├─ BundleResolver (warm WASM → warm Deno → cold fetch → S3/inline)
//!  ├─ SecretsClient (LRU cache, 30 s TTL)
//!  └─ ExecutionRunner::run()
//!       ├─ schema validation (input JSON Schema, if configured)
//!       ├─ TraceEmitter::post_lifecycle("start")
//!       ├─ IsolatePool::execute()   (Deno) — warm V8 isolate, function affinity
//!       │   OR WasmPool::execute()  (WASM) — Wasmtime AOT + fuel limit
//!       └─ TraceEmitter::emit_logs()  — fire-and-forget ctx.log() + execution_end span
//! ```

pub mod agent;
pub mod bundle;
pub mod config;
pub mod dispatch;
pub mod engine;
pub mod execute;
pub mod schema;
pub mod secrets;
pub mod state;
pub mod trace;

// Convenience re-exports at crate root.
pub use state::AppState;
