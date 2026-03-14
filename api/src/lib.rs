//! `api` library crate.
//!
//! Exposes the full module tree so the monolithic `server` binary can link
//! against this crate instead of spawning a separate `api` process.

pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod logs;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod secrets;
pub mod services;
pub mod types;
pub mod validation;

// Convenience re-exports at crate root.
pub use app::{AppState, build_cors, create_app};
