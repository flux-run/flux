//! Executor module — exposes the two SQL execution paths.
//!
//! ## Single execution (`db_executor::execute`)
//!
//! The standard path for all mutations and single-level SELECTs. Wraps the user
//! query in an explicit Postgres transaction with four ordered steps:
//!
//! 1. `SET LOCAL search_path` — tenant isolation
//! 2. `SET LOCAL statement_timeout` — Postgres-level query cancellation
//! 3. Pre-read (UPDATE only) — `SELECT … FOR UPDATE` to capture `before_state`
//! 4. User query + `state_mutations` write (atomic)
//!
//! ## Batched execution (`batched::execute_batched`)
//!
//! Used when the query compiler emits a [`crate::compiler::query_compiler::CompileResult::Batched`]
//! plan — i.e. when the nested selector depth is ≥ `BATCH_DEPTH_THRESHOLD`.
//! Instead of one deeply nested SQL CTE (which can produce a cartesian explosion),
//! the executor fetches each child level separately in Rust and joins the results
//! in memory. This trades a single complex query for N simple ones.
pub mod batched;
pub mod db_executor;
pub use batched::execute_batched;
pub use db_executor::{execute, MutationContext};
