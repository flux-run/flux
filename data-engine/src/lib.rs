//! `data-engine` library crate.
//!
//! Exposes the full module tree so the standalone `data-engine` binary and the
//! monolithic `server` binary can both link against this crate.

pub mod api;
pub mod cache;
pub mod compiler;
pub mod config;
pub mod cron;
pub mod db;
pub mod engine;
pub mod events;
pub mod executor;
pub mod file_engine;
pub mod hooks;
pub mod policy;
pub mod query_guard;
pub mod retention;
pub mod router;
pub mod state;
pub mod telemetry;
pub mod transform;
