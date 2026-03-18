//! `runtime` library crate — executes JavaScript handlers in embedded V8 isolates.
//!
//! This crate is responsible for:
//! - preparing artifacts,
//! - running user code inside reused isolates,
//! - exposing Rust-owned host I/O ops (for example intercepted `fetch`).

pub mod artifact;
pub mod deno_runtime;
pub mod http_runtime;
pub mod isolate_pool;
pub mod server_client;

pub use artifact::{
	RuntimeArtifact,
	RuntimeSubmitRequest,
	build_artifact,
	build_artifact_from_file,
	load_built_artifact_from_file,
	sha256_hex,
};
pub use http_runtime::{
	HttpRuntimeConfig,
	run_http_runtime,
};
pub use deno_runtime::{BootExecutionResult, JsIsolate, boot_runtime_artifact};
