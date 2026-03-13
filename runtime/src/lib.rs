//! `runtime` library crate.
//!
//! Exposes the full module tree so `src/main.rs` (the standalone binary) and
//! the monolithic `server` binary can both compile against it.

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
