//! `runtime` library crate — sandboxed user-code execution.
//!
//! ## Mental model
//!
//! Flux is `node` but written in Rust. User runs their existing JS/TS code unchanged:
//!
//! ```bash
//! flux serve index.js
//! ```
//!
//! Flux boots a V8 isolate, loads the file, and runs it exactly like Node would —
//! except Flux **owns every IO primitive in Rust**. `fetch()` is a Rust function.
//! The DB client is a Rust function. Every outbound call crosses from V8 into Rust
//! before hitting the network.
//!
//! ## Checkpoint recording
//!
//! At every IO boundary crossing, Flux records a checkpoint:
//! ```text
//! call_index=0  fetch POST https://stripe.com  → recorded (request + response)
//! call_index=1  db INSERT users               → recorded (query + result)
//! call_index=2  fetch POST https://email.co   → recorded (request + response)
//! ```
//!
//! This makes any execution fully replayable: re-run the same code, inject recorded
//! responses by `call_index` instead of hitting the network.
//!
//! ## Execution path
//!
//! ```text
//! incoming HTTP request
//!        ↓
//! create execution_record (status='running')
//!        ↓
//! IsolatePool::execute() — warm V8 isolate
//!   fetch() → Rust op_http_fetch → record checkpoint → return response
//!   db.*    → Rust op_db_*       → record checkpoint → return result
//!        ↓
//! update execution_record (status='ok'|'error', duration_ms)
//! ```

pub mod bundle;
pub mod checkpoint;
pub mod checkpoint_store;
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
pub use checkpoint::{
    BoundaryType, CallIndex, Checkpoint, CheckpointStore, ExecutionContext, ExecutionMode,
};
