//! `gateway` library crate.
//!
//! Exposes the full module tree so `src/bin/*` helpers and the monolithic
//! `server` binary can both link against this crate.

pub mod auth;
pub mod config;
pub mod forward;
pub mod handlers;
pub mod rate_limit;
pub mod router;
pub mod snapshot;
pub mod state;
pub mod trace;

// Convenience re-exports at crate root.
pub use router::create_router;
pub use state::GatewayState;
