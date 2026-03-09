//! Query complexity guard and execution timeout wrapper.
//!
//! ## Complexity scoring
//!
//! Every incoming query is assigned a score before any database work begins.
//! Queries whose score exceeds the configured ceiling are rejected immediately
//! with HTTP 400 — no schema lookup, no compilation, no execution.
//!
//! ### Scoring model
//!
//! | Component | Score |
//! |---|---|
//! | Each filter clause | +2 |
//! | Nested selector at depth 1 | +10 |
//! | Nested selector at depth 2 | +20 |
//! | Nested selector at depth N | +10 × 2^(N-1) |
//!
//! The exponential depth penalty reflects the actual cost growth:
//! - Depth 1 → correlated lateral (cheap)
//! - Depth 2–3 → CTE aggregation (moderate)
//! - Depth ≥ 4 → batched execution (multiple round-trips)
//!
//! ### Example scores
//!
//! | Request | Score |
//! |---|---|
//! | `SELECT *` | 0 |
//! | `SELECT * WHERE a=1 AND b=2` | 4 |
//! | `users?select=posts(id)` | 10 |
//! | `users?select=posts(id,comments(id))` | 30 |
//! | `users?select=posts(id,comments(id,likes(id)))` | 70 |
//! | depth 5 chain | 150 |
//!
//! ## Timeout
//!
//! All database operations (execution and batched child queries) are wrapped
//! in a `tokio::time::timeout`.  Exceeding the limit yields HTTP 408.

use std::time::Duration;

use crate::compiler::query_compiler::QueryRequest;
use crate::compiler::relational::{parse_selectors, ColumnSelector};
use crate::engine::error::EngineError;

// ─── Guard ────────────────────────────────────────────────────────────────────

pub struct QueryGuard {
    /// Requests whose complexity score exceeds this ceiling are rejected.
    pub max_complexity: u64,
    /// Maximum time the full execution phase may take before yielding 408.
    pub timeout: Duration,
}

impl QueryGuard {
    pub fn new(max_complexity: u64, timeout_ms: u64) -> Self {
        Self {
            max_complexity,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    /// Score the request and return `Err(QueryTooComplex)` if it exceeds the
    /// ceiling.  Returns the computed score so the caller can log it.
    pub fn check_complexity(&self, req: &QueryRequest) -> Result<u64, EngineError> {
        let score = score_request(req);
        if score > self.max_complexity {
            return Err(EngineError::QueryTooComplex {
                score,
                limit: self.max_complexity,
            });
        }
        Ok(score)
    }

    /// Wrap a future in the configured timeout.
    ///
    /// Maps `tokio::time::error::Elapsed` → [`EngineError::QueryTimeout`].
    pub async fn with_timeout<F, T>(&self, fut: F) -> Result<T, EngineError>
    where
        F: std::future::Future<Output = Result<T, EngineError>>,
    {
        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| EngineError::QueryTimeout)?
    }
}

// ─── Scorer ──────────────────────────────────────────────────────────────────

/// Compute the complexity score for a [`QueryRequest`].
fn score_request(req: &QueryRequest) -> u64 {
    // Each filter term costs 2 (it adds one WHERE predicate + bind param).
    let filter_score: u64 = req.filters.as_deref().map_or(0, |f| f.len() as u64 * 2);

    // Selector tree cost — exponential by nesting depth.
    let selector_score: u64 = req
        .columns
        .as_ref()
        .map_or(0, |cols| {
            parse_selectors(cols)
                .iter()
                .map(|s| score_selector(s, 1))
                .sum()
        });

    filter_score + selector_score
}

/// Score one selector node.  Flat columns cost 0; nested selectors pay an
/// exponential penalty based on their depth in the tree.
fn score_selector(sel: &ColumnSelector, depth: u32) -> u64 {
    match sel {
        ColumnSelector::Flat(_) => 0,
        ColumnSelector::Nested { cols, .. } => {
            // Cap the shift at 15 to avoid overflow on absurd depths.
            let own = 10u64.saturating_mul(1u64 << (depth - 1).min(15));
            let children: u64 = cols.iter().map(|c| score_selector(c, depth + 1)).sum();
            own + children
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn req_with_cols(cols: &[&str]) -> QueryRequest {
        QueryRequest {
            database: "main".into(),
            table: "users".into(),
            operation: "select".into(),
            columns: Some(cols.iter().map(|s| s.to_string()).collect()),
            filters: None,
            data: None,
            limit: None,
            offset: None,
        }
    }

    #[test]
    fn flat_select_is_zero() {
        assert_eq!(score_request(&req_with_cols(&["id", "name"])), 0);
    }

    #[test]
    fn depth_1_scores_10() {
        // "posts(id)" → one nested selector at depth 1
        assert_eq!(score_request(&req_with_cols(&["posts(id)"])), 10);
    }

    #[test]
    fn depth_2_scores_30() {
        // "posts(id,comments(id))" → depth-1 (10) + depth-2 child (20) = 30
        assert_eq!(score_request(&req_with_cols(&["posts(id,comments(id))"])), 30);
    }

    #[test]
    fn depth_3_scores_70() {
        // depth-1 (10) + depth-2 (20) + depth-3 (40) = 70
        assert_eq!(
            score_request(&req_with_cols(&["posts(id,comments(id,likes(id)))"])),
            70,
        );
    }

    #[test]
    fn multiple_top_level_nested_scores_correctly() {
        // "posts(id)" + "tags(id)" → 10 + 10 = 20
        assert_eq!(score_request(&req_with_cols(&["posts(id)", "tags(id)"])), 20);
    }

    #[test]
    fn filters_add_two_each() {
        use crate::compiler::query_compiler::Filter;
        let mut r = req_with_cols(&[]);
        r.filters = Some(vec![
            Filter { column: "a".into(), op: "eq".into(), value: serde_json::Value::Null },
            Filter { column: "b".into(), op: "gt".into(), value: serde_json::Value::Null },
        ]);
        assert_eq!(score_request(&r), 4);
    }

    #[test]
    fn guard_allows_below_ceiling() {
        let g = QueryGuard::new(1000, 30_000);
        let req = req_with_cols(&["posts(id,comments(id))"]);
        assert!(g.check_complexity(&req).is_ok());
    }

    #[test]
    fn guard_rejects_above_ceiling() {
        // Set ceiling at 5 — even a depth-1 nested selector (score=10) is rejected.
        let g = QueryGuard::new(5, 30_000);
        let req = req_with_cols(&["posts(id)"]);
        assert!(matches!(
            g.check_complexity(&req),
            Err(EngineError::QueryTooComplex { .. })
        ));
    }
}
