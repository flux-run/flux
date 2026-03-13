//! `EventPayload` — the payload pushed to queues for `on.*` event handlers.
//!
//! In TypeScript schema files:
//! ```ts
//! on: {
//!   insert: ["send_welcome_email", "update_user_count"],
//!   update: (payload) => payload.row.role !== payload.prev?.role
//!             ? ["notify_role_change"] : [],
//! }
//! ```
//! At runtime, Rust pushes `EventPayload<Row>` to the queue for each
//! function listed in the `on.*` handler. Queue workers deserialize it
//! back to the typed payload.
//!
//! The `on` handler is stored as either:
//! - `OnSpec::Static(Vec<String>)` — always push these functions
//! - `OnSpec::Conditional(RuleExpr, Vec<String>)` — push only if condition holds

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::rules::RuleExpr;

/// The payload shape pushed onto the queue for `on.*` event handlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPayload {
    /// CRUD operation: "insert" | "update" | "delete" | "select"
    pub operation: String,
    /// Table name
    pub table:     String,
    /// The current (new) row
    pub row:       Value,
    /// Previous row value (null for inserts)
    pub prev:      Value,
    /// Original mutation input before before-hooks ran
    pub input:     Value,
    /// Auth context (serialized subset — no secrets)
    pub ctx:       EventCtx,
    /// Trace request ID for correlation
    pub request_id: Option<String>,
}

/// Minimal auth context included in event payloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventCtx {
    pub user_id:   Option<String>,
    pub user_role: Option<String>,
}

impl EventPayload {
    pub fn new(
        operation: impl Into<String>,
        table:     impl Into<String>,
        row:       Value,
        prev:      Value,
        input:     Value,
        user_id:   Option<String>,
        user_role: Option<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            table:     table.into(),
            row,
            prev,
            input,
            ctx: EventCtx { user_id, user_role },
            request_id,
        }
    }
}

// ── On-handler spec ───────────────────────────────────────────────────────────

/// Compiled `on.*` spec — stored in `flux.schema_events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum OnSpec {
    /// Always push to these function queues.
    Static { functions: Vec<String> },
    /// Push only if the condition evaluates to true.
    Conditional {
        cond:      RuleExpr,
        functions: Vec<String>,
    },
}

impl OnSpec {
    /// Resolve which functions to push given the current event payload.
    pub fn resolve_functions(
        &self,
        ctx: &super::eval::EvalCtx,
        row: &Value,
        prev: &Value,
        input: &Value,
    ) -> Vec<String> {
        match self {
            Self::Static { functions } => functions.clone(),
            Self::Conditional { cond, functions } => {
                if cond.evaluate(ctx, row, prev, input) {
                    functions.clone()
                } else {
                    vec![]
                }
            }
        }
    }

    pub fn from_json(v: &Value) -> anyhow::Result<Self> {
        serde_json::from_value(v.clone()).map_err(Into::into)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{eval::{EvalCtx, UserCtx}, rules::{RuleExpr, ValueExpr}};
    use serde_json::{json, Value};

    fn ctx() -> EvalCtx {
        EvalCtx { user: UserCtx::default(), request_id: None }
    }

    #[test]
    fn static_spec_always_resolves() {
        let spec = OnSpec::Static { functions: vec!["send_email".into()] };
        let fns = spec.resolve_functions(&ctx(), &json!({}), &Value::Null, &json!({}));
        assert_eq!(fns, vec!["send_email"]);
    }

    #[test]
    fn conditional_spec() {
        // Push "notify_role_change" only when row.role != prev.role
        let spec = OnSpec::Conditional {
            cond: RuleExpr::Ne {
                left:  ValueExpr::RowField  { field: "role".into() },
                right: ValueExpr::PrevField { field: "role".into() },
            },
            functions: vec!["notify_role_change".into()],
        };

        let row  = json!({ "role": "admin" });
        let prev = json!({ "role": "user"  });
        assert_eq!(
            spec.resolve_functions(&ctx(), &row, &prev, &json!({})),
            vec!["notify_role_change"]
        );

        // Same role → no functions
        let prev_same = json!({ "role": "admin" });
        assert!(spec.resolve_functions(&ctx(), &row, &prev_same, &json!({})).is_empty());
    }
}
