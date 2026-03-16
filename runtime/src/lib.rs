//! `runtime` library crate — prepares JavaScript artifacts for the server.
//!
//! This crate no longer executes user code.
//! Its only job is to:
//! - accept a JavaScript source file or string,
//! - compute a deterministic SHA-256 content hash,
//! - package the script into a payload the server can store and run.

pub mod artifact;

pub use artifact::{
	RuntimeArtifact,
	RuntimeSubmitRequest,
	build_artifact,
	build_artifact_from_file,
	sha256_hex,
};
