//! `data-engine` library crate — the sole writer to project databases.
//!
//! ## Mental model
//!
//! Data Engine sits between functions and Postgres. **It is the only writer.**
//! This is non-negotiable — controlling all writes enables:
//!
//! - **Atomic mutation recording**: every INSERT/UPDATE/DELETE produces a
//!   `flux_internal.state_mutations` row in the same transaction.
//! - **State history**: the full before/after state of every row is captured,
//!   including `changed_fields` (sorted key list).
//! - **Deterministic replay**: given a `request_id`, re-execute all mutations
//!   against a snapshot to reconstruct any historical state.
//! - **Blame**: `actor_id` + `span_id` link every mutation to the authenticated
//!   user and the specific runtime span that caused it.
//!
//! ## Architecture
//!
//! ```text
//! POST /db/query (HTTP)
//!        ↓
//! QueryPipeline::run()          ← orchestrates all steps (see engine/pipeline.rs)
//!  ├─ AuthContext               ← request_id, user_id, is_replay flag
//!  ├─ DbRouter                  ← schema_name (tenant isolation)
//!  ├─ QueryGuard                ← complexity ceiling + nesting depth
//!  ├─ SchemaCache               ← column metadata + relationships (L1 Moka)
//!  ├─ QueryCompiler             ← JSON query API → SQL + params (L2 plan cache)
//!  └─ db_executor::execute()    ← transaction: search_path + timeout + pre-read + user query + state_mutations
//! ```

pub mod schema;
pub mod api;
pub mod cache;
pub mod compiler;
pub mod config;
pub mod cron;
pub mod db;
pub mod engine;
pub mod executor;
pub mod query_guard;
pub mod retention;
pub mod router;
pub mod state;
pub mod telemetry;
pub mod transform;
