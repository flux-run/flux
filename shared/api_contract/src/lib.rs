//! `api_contract` — single source of truth for all Flux HTTP API types.
//!
//! Every request payload and response type shared between:
//!   - `api` crate (server — parses requests, serialises responses, queries DB)
//!   - `cli` crate (client — builds requests, deserialises responses)
//!   - Dashboard (TypeScript — generated via `make types`)
//!
//! # Feature flags
//! - `server` — enables `sqlx::FromRow` on row types (for the `api` crate)
//! - `ts`     — enables `ts_rs::TS` for TypeScript codegen (`make types`)
//!
//! # Adding a new type
//! 1. Add it to the appropriate module below.
//! 2. Derive `Serialize, Deserialize` (both — server serialises, client deserialises).
//! 3. If the type is fetched from the DB via `query_as`, add:
//!    `#[cfg_attr(feature = "server", derive(sqlx::FromRow))]`
//! 4. Rebuild and run `make types` to regenerate the TypeScript bindings.

pub mod api_keys;
pub mod db_migrate;
pub mod deployments;
pub mod environments;
pub mod events;
pub mod functions;
pub mod gateway;
pub mod queue;
pub mod schedules;
pub mod secrets;

mod tests;
