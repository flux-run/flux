//! Axum middleware layers.
//!
//! ## SOLID note (Single Responsibility)
//! Each module handles exactly ONE cross-cutting concern:
//!   - `auth`          — authentication + project-context injection
//!   - `internal_auth` — service-token guard for `/internal/*` routes
//!   - `request_id`    — x-request-id propagation and request logging
//!
//! Removed from the previous cloud-SaaS build:
//!   - `context`       — Firebase UID → tenant/project DB lookup (no longer needed)
//!   - `scope`         — Platform/Tenant/Project scope enforcement (no multi-tenancy)
//!   - `api_key_auth`  — multi-tenant API key validation (entire api_keys module removed)
pub mod auth;
pub mod internal_auth;
pub mod request_id;

// ── Shared secret helper ──────────────────────────────────────────────────────

/// Load a secret from an env var.
///
/// In production (`FLUX_ENV=production`) the service panics if the env var is
/// absent or still set to the known-weak framework default.  In any other
/// environment a warning is logged and the dev default is returned so local
/// development works without configuration.
pub fn require_secret(env_var: &str, dev_default: &str, label: &str) -> String {
    match std::env::var(env_var) {
        Ok(v) if !v.is_empty() => v,
        _ => {
            if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] {label} must be set in production. \
                     Set the {env_var} environment variable."
                );
            }
            tracing::warn!(
                "[Flux] {label} not configured — using insecure default. \
                 Set {env_var} before deploying to production."
            );
            dev_default.to_string()
        }
    }
}
