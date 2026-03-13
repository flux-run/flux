//! `queue` library crate.
//!
//! Exposes the full module tree so the standalone `queue` binary and the
//! monolithic `server` binary can both link against this crate.

pub mod api;
pub mod config;
pub mod db;
pub mod models;
pub mod queue;
pub mod services;
pub mod state;
pub mod worker;
