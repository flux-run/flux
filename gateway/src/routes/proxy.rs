//! Backward-compatible thin shim.
//!
//! All tenant function routing has been moved to [`crate::routes::tenant_router`].
//! This module re-exports the handler under the old name so that existing
//! references in `router.rs` continue to compile without changes.

pub use crate::routes::tenant_router::tenant_route_handler as proxy_handler;
