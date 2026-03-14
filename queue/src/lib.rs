//! `queue` library crate — durable async job processor.
//!
//! ## Mental model
//!
//! Queue is a durable async job processor. It **never executes user code directly** —
//! all execution is delegated to the Runtime service over HTTP (`POST /execute`).
//! Every job produces a full execution record via [`worker::span_emitter::QueueSpanEmitter`]
//! writing spans to `flux.platform_logs` through `ApiDispatch::write_log`.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │  POST /jobs (HTTP)   →  job_service::create_job                  │
//! │                              ↓                                   │
//! │                      flux.jobs  (Postgres)                       │
//! │                              ↓                                   │
//! │  poller (loop)       ← fetch_and_lock_jobs (FOR UPDATE SKIP LOCKED) │
//! │       ↓  (semaphore-capped tokio::spawn per job)                 │
//! │  executor::execute                                               │
//! │       ├─ QueueSpanEmitter → flux.platform_logs (fire-and-forget) │
//! │       └─ POST {runtime_url}/execute → Runtime                    │
//! │              (all user-code spans also go to platform_logs)      │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Crate dual-use
//!
//! Exposes the full module tree so both the standalone `queue` binary and the
//! monolithic `server` binary can link against this crate.

pub mod api;
pub mod config;
pub mod db;
pub mod dispatch;
pub mod models;
pub mod queue;
pub mod services;
pub mod state;
pub mod worker;
