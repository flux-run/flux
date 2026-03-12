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
