/// Request context injected by auth middleware.
pub mod context;
/// Shared response types (type alias for handler returns).
pub mod response;

// `scope.rs` (Platform/Tenant/Project enum) removed:
// the standalone framework has no multi-tenant scoping layers.

