pub mod invalidation;
pub mod manager;
pub use manager::CacheManager;

// ─── Two-layer query plan cache ───────────────────────────────────────────────
//
// Layer 1 — Schema cache
//   Caches (col_meta, relationships) per (tenant, project, schema, table).
//   Eliminates two DB round-trips on the hot read path.
//   TTL: 60 s; explicitly invalidated on DDL mutations.
//
// **Layer 2 — Plan cache**
// Caches the compiled SQL template for SELECT queries, keyed by request shape
// + policy fingerprint.  On a cache hit the caller rebuilds the bind parameters
// directly from the request (O(n) walk of the filter list) instead of running
// the full compiler pipeline.
// TTL: 300 s; invalidated together with Layer 1.
//
// ## Why this matters
//
// Each `POST /db/query` would otherwise pay:
//   1. `TransformEngine::load_columns`   — 1 DB round-trip
//   2. `load_relationships`              — 1 DB round-trip
//   3. `QueryCompiler::compile`          — CPU: parse selectors, resolve cols,
//                                          expand lateral subqueries, build WHERE
//
// With both caches warm, steps 1-2 become an in-memory HashMap lookup and
// step 3 becomes a filter-list walk to collect bind values.

use std::time::Duration;

use moka::sync::Cache;

use crate::compiler::relational::{parse_selectors, ColumnSelector, RelationshipDef};
use crate::compiler::query_compiler::QueryRequest;
use crate::policy::PolicyResult;
use crate::transform::ColumnMeta;

/// A fully compiled SELECT query plan.
/// The plan cache stores this instead of a bare SQL string so the transform
/// fast-path can skip the col_meta walk when the table has no file columns.
#[derive(Clone)]
pub struct QueryPlan {
    /// SQL template with `$N` placeholders.
    pub sql: String,
    /// `true` if any column in this table has `fb_type = "file"`.
    /// When `false`, the transform engine can be bypassed entirely on cache hits.
    pub has_file_cols: bool,
    /// `true` when this plan was compiled as a batched execution (depth ≥
    /// [`BATCH_DEPTH_THRESHOLD`]).  On a cache hit the handler reconstructs
    /// the [`BatchedPlan`] from the in-memory schema cache and executes it.
    pub is_batched: bool,
}

// ─── TTLs ─────────────────────────────────────────────────────────────────────

/// Schema metadata (col_meta + relationships) lives for 60 s.
/// Most schema changes are rare DDL operations; explicit invalidation covers
/// those cases and the TTL acts as a safety net.
pub const SCHEMA_TTL: Duration = Duration::from_secs(60);

/// Compiled SQL templates live for 5 min.
pub const PLAN_TTL: Duration = Duration::from_secs(300);

const CACHE_MAX_CAPACITY: u64 = 10_000;

// ─── Schema cache ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SchemaCacheEntry {
    pub col_meta: Vec<ColumnMeta>,
    pub relationships: Vec<RelationshipDef>,
}

/// Moka LRU cache keyed by `"{schema}:{table}"`.
pub type SchemaCache = Cache<String, SchemaCacheEntry>;

/// Construct a fresh [`SchemaCache`] with the standard TTL and capacity.
pub fn build_schema_cache() -> SchemaCache {
    Cache::builder()
        .max_capacity(CACHE_MAX_CAPACITY)
        .time_to_live(SCHEMA_TTL)
        .build()
}

/// Build the string key for one specific table.
///
/// Format: `"{schema}:{table}"`
pub fn schema_key(schema: &str, table: &str) -> String {
    format!("{}:{}", schema, table)
}

/// Build the prefix used to evict all entries for a schema at once.
///
/// Format: `"{schema}:"`
pub fn schema_prefix(schema: &str) -> String {
    format!("{}:", schema)
}

// ─── Plan cache ───────────────────────────────────────────────────────────────

/// Stable fingerprint of a SELECT query's shape.
///
/// Two requests are "same shape" iff they produce identical SQL — the only
/// difference between them being the values bound to `$N` placeholders.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PlanKey {
    /// Postgres schema name.
    pub schema: String,
    /// Table name.
    pub table: String,
    /// Alphabetically-sorted flat column names, comma-joined.
    /// `"*"` when no columns were specified.
    pub columns: String,
    /// Alphabetically-sorted nested-selector aliases, comma-joined.
    /// `""` when there are no nested selectors.
    pub nested_aliases: String,
    /// Sorted `"col:op"` pairs, e.g. `"age:gt,role:eq"`.
    /// `""` when there are no filters.
    pub filter_shape: String,
    /// Whether the request carries an OFFSET clause (affects SQL structure).
    pub has_offset: bool,
    /// `"{sorted_allowed_cols}|{row_condition_sql}"`.
    /// Ensures plans aren't reused across policies.
    pub policy_fingerprint: String,
}

/// Moka LRU cache: plan key → [`QueryPlan`].
pub type PlanCache = Cache<PlanKey, QueryPlan>;

/// Construct a fresh [`PlanCache`] with the standard TTL and capacity.
pub fn build_plan_cache() -> PlanCache {
    Cache::builder()
        .max_capacity(CACHE_MAX_CAPACITY)
        .time_to_live(PLAN_TTL)
        .build()
}

// ─── Key builders ─────────────────────────────────────────────────────────────

/// Build a [`PlanKey`] from a SELECT request and its evaluated policy.
///
/// Called on every SELECT — used for cache lookup before compilation and for
/// cache insertion after a compile miss.
pub fn build_plan_key(
    schema: &str,
    req: &QueryRequest,
    policy: &PolicyResult,
) -> PlanKey {
    // Single pass: collect flat column names and recursive nested fingerprints.
    let (mut flat, mut nested_fps) = (Vec::<String>::new(), Vec::<String>::new());
    if let Some(ref cols) = req.columns {
        for sel in parse_selectors(cols) {
            match sel {
                ColumnSelector::Flat(c) => flat.push(c),
                // Use the recursive fingerprint so that
                // `posts(id,comments(id,body))` ≠ `posts(id)` in the cache.
                sel @ ColumnSelector::Nested { .. } => nested_fps.push(sel.fingerprint()),
            }
        }
    }
    flat.sort_unstable();
    nested_fps.sort_unstable();

    let columns = if flat.is_empty() { "*".to_string() } else { flat.join(",") };
    let nested_aliases = nested_fps.join(",");
    let mut pairs: Vec<String> = req
        .filters
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|f| format!("{}:{}", f.column, f.op))
        .collect();
    pairs.sort_unstable();
    let filter_shape = pairs.join(",");

    // Policy fingerprint: sorted allowed columns + row condition expression.
    let mut allowed = policy.allowed_columns.clone();
    allowed.sort_unstable();
    let policy_fingerprint = format!(
        "{}|{}",
        allowed.join(","),
        policy.row_condition_sql.as_deref().unwrap_or(""),
    );

    PlanKey {
        schema: schema.to_owned(),
        table: req.table.clone(),
        columns,
        nested_aliases,
        filter_shape,
        has_offset: req.offset.is_some(),
        policy_fingerprint,
    }
}

// ─── Param extraction (SELECT fast path) ──────────────────────────────────────

/// Reconstruct the bind-parameter list for a SELECT query whose SQL template
/// was retrieved from the plan cache.
///
/// The ordering **must** match `compile_select` in `query_compiler.rs`:
///
///   1. Policy row-condition params (already embedded in the SQL template at
///      positions `$1 … $N_rls`).
///   2. Filter params — one per filter, in request order, skipping `is_null` /
///      `not_null` ops (which generate no placeholder).
///   3. `LIMIT $N` value.
///   4. `OFFSET $N` value — only when `req.offset` is `Some`.
pub fn extract_select_params(
    req: &QueryRequest,
    policy: &PolicyResult,
    default_limit: i64,
    max_limit: i64,
) -> Vec<serde_json::Value> {
    let mut params: Vec<serde_json::Value> = policy.row_condition_params.clone();

    if let Some(filters) = &req.filters {
        for f in filters {
            if f.op != "is_null" && f.op != "not_null" {
                params.push(f.value.clone());
            }
        }
    }

    let effective_limit = match req.limit {
        Some(l) => l.min(max_limit).max(1),
        None => default_limit,
    };
    params.push(serde_json::Value::Number(effective_limit.into()));

    if let Some(offset) = req.offset {
        params.push(serde_json::Value::Number(offset.into()));
    }

    params
}
