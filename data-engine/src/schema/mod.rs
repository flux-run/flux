//! Schema-driven rule and transform AST types.
//!
//! Users write rules and transforms in TypeScript (`*.schema.ts`).
//! `flux db push` compiles them to JSON AST stored in Postgres.
//! This module evaluates those ASTs in Rust — zero JS on the hot path.
//!
//! ## Modules
//! - [`rules`]     — `RuleExpr` AST: row-level and column-level access rules
//! - [`hooks`]     — `TransformExpr` AST: before/after mutation transforms
//! - [`eval`]      — `EvalCtx` passed to both evaluators at runtime
//! - [`events`]    — `EventPayload` pushed to queues for `on.*` handlers

pub mod eval;
pub mod events;
pub mod hooks;
pub mod rules;
