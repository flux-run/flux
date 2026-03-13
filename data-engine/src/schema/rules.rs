//! `RuleExpr` — typed AST for row-level and column-level access rules.
//!
//! Compiled from TypeScript predicates by `flux db push`, stored as JSON in
//! `flux.schema_rules`, and evaluated here in Rust with zero JS overhead.
//!
//! ## TypeScript source → AST
//! ```ts
//! // users.schema.ts
//! rules: {
//!   select: (ctx, row) => ctx.user.id === row.id || ctx.user.role === 'admin',
//!   insert: (ctx) => authenticated(ctx),
//!   update: adminOrOwner,
//!   delete: adminOnly,
//! }
//! ```
//! `flux db push` compiles the arrow function body to a `RuleExpr` JSON tree
//! and stores it. At request time, [`RuleExpr::evaluate`] is called.
//!
//! ## Supported constructs
//! - `===` / `!==` / `<` / `<=` / `>` / `>=` comparisons
//! - `&&` / `||` / `!` logical operators
//! - `ctx.user.*` path reads (resolved via [`super::eval::EvalCtx`])
//! - `row.*` / `prev.*` / `input.*` field reads
//! - String / number / boolean / null literals
//! - `.includes()` → `RuleExpr::In`
//! - Inlined shared predicates (e.g. `adminOrOwner` → inlined at push time)

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::eval::EvalCtx;

// ── Value expression ──────────────────────────────────────────────────────────

/// A leaf value resolvable at runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ValueExpr {
    /// `ctx.user.id` → path = ["user", "id"]
    CtxPath { path: Vec<String> },
    /// `row.user_id`
    RowField { field: String },
    /// `prev.status`  (only available in update/delete rules)
    PrevField { field: String },
    /// `input.email`  (available in insert/update rules)
    InputField { field: String },
    /// String literal `"admin"`
    Str { value: String },
    /// Numeric literal `42`
    Num { value: f64 },
    /// Boolean literal `true` / `false`
    Bool { value: bool },
    /// `null`
    Null,
}

impl ValueExpr {
    pub fn resolve(
        &self,
        ctx: &EvalCtx,
        row: &Value,
        prev: &Value,
        input: &Value,
    ) -> Value {
        match self {
            Self::CtxPath { path }    => ctx.resolve(path),
            Self::RowField { field }  => row.get(field).cloned().unwrap_or(Value::Null),
            Self::PrevField { field } => prev.get(field).cloned().unwrap_or(Value::Null),
            Self::InputField { field }=> input.get(field).cloned().unwrap_or(Value::Null),
            Self::Str { value }       => Value::String(value.clone()),
            Self::Num { value }       => Value::from(*value),
            Self::Bool { value }      => Value::Bool(*value),
            Self::Null                => Value::Null,
        }
    }
}

// ── Rule expression ───────────────────────────────────────────────────────────

/// A boolean-valued expression compiled from a TypeScript rule predicate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum RuleExpr {
    // ── Logical ──────────────────────────────────────────────────────────────
    And { operands: Vec<RuleExpr> },
    Or  { operands: Vec<RuleExpr> },
    Not { operand: Box<RuleExpr> },

    // ── Comparisons ──────────────────────────────────────────────────────────
    Eq  { left: ValueExpr, right: ValueExpr },
    Ne  { left: ValueExpr, right: ValueExpr },
    Lt  { left: ValueExpr, right: ValueExpr },
    Le  { left: ValueExpr, right: ValueExpr },
    Gt  { left: ValueExpr, right: ValueExpr },
    Ge  { left: ValueExpr, right: ValueExpr },

    // ── Membership ───────────────────────────────────────────────────────────
    /// `["admin","mod"].includes(ctx.user.role)` or `ctx.user.role.includes(set)`
    In  { value: ValueExpr, set: Vec<ValueExpr> },

    // ── Terminal ─────────────────────────────────────────────────────────────
    /// Always allow (compiled from `allowAll`)
    Allow,
    /// Always deny (compiled from `denyAll`)
    Deny,
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Errors that can occur during rule evaluation.
#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("type mismatch in rule: cannot compare {0} and {1}")]
    TypeMismatch(String, String),
}

impl RuleExpr {
    /// Evaluate this rule expression.
    ///
    /// - `ctx`   — authenticated user context
    /// - `row`   — current row being accessed (JSON object)
    /// - `prev`  — previous row value (for UPDATE/DELETE; pass `Value::Null` otherwise)
    /// - `input` — incoming mutation payload (for INSERT/UPDATE; pass `Value::Null` for SELECT/DELETE)
    pub fn evaluate(
        &self,
        ctx: &EvalCtx,
        row: &Value,
        prev: &Value,
        input: &Value,
    ) -> bool {
        match self {
            Self::Allow => true,
            Self::Deny  => false,

            Self::And { operands } =>
                operands.iter().all(|e| e.evaluate(ctx, row, prev, input)),
            Self::Or  { operands } =>
                operands.iter().any(|e| e.evaluate(ctx, row, prev, input)),
            Self::Not { operand }  =>
                !operand.evaluate(ctx, row, prev, input),

            Self::Eq { left, right } => {
                let l = left.resolve(ctx, row, prev, input);
                let r = right.resolve(ctx, row, prev, input);
                json_eq(&l, &r)
            }
            Self::Ne { left, right } => {
                let l = left.resolve(ctx, row, prev, input);
                let r = right.resolve(ctx, row, prev, input);
                !json_eq(&l, &r)
            }
            Self::Lt { left, right } =>
                json_cmp(left.resolve(ctx, row, prev, input),
                         right.resolve(ctx, row, prev, input))
                    .map(|o| o.is_lt()).unwrap_or(false),
            Self::Le { left, right } =>
                json_cmp(left.resolve(ctx, row, prev, input),
                         right.resolve(ctx, row, prev, input))
                    .map(|o| o.is_le()).unwrap_or(false),
            Self::Gt { left, right } =>
                json_cmp(left.resolve(ctx, row, prev, input),
                         right.resolve(ctx, row, prev, input))
                    .map(|o| o.is_gt()).unwrap_or(false),
            Self::Ge { left, right } =>
                json_cmp(left.resolve(ctx, row, prev, input),
                         right.resolve(ctx, row, prev, input))
                    .map(|o| o.is_ge()).unwrap_or(false),

            Self::In { value, set } => {
                let v = value.resolve(ctx, row, prev, input);
                set.iter().any(|s| json_eq(&v, &s.resolve(ctx, row, prev, input)))
            }
        }
    }

    /// Deserialize from JSON stored in `flux.schema_rules`.
    pub fn from_json(v: &Value) -> anyhow::Result<Self> {
        serde_json::from_value(v.clone()).map_err(Into::into)
    }

    /// Serialize to JSON for storage.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("RuleExpr serialization never fails")
    }
}

// ── JSON comparison helpers ───────────────────────────────────────────────────

fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null)   => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Number(x), Value::Number(y)) =>
            x.as_f64().zip(y.as_f64()).map(|(a, b)| (a - b).abs() < f64::EPSILON)
             .unwrap_or(false),
        _ => false,
    }
}

fn json_cmp(a: Value, b: Value) -> Option<std::cmp::Ordering> {
    let an = a.as_f64()?;
    let bn = b.as_f64()?;
    an.partial_cmp(&bn)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_admin() -> EvalCtx {
        EvalCtx {
            user: super::super::eval::UserCtx {
                id:    Some("u1".into()),
                role:  Some("admin".into()),
                email: Some("admin@example.com".into()),
                claims: Value::Null,
            },
            request_id: None,
        }
    }

    fn ctx_user(id: &str) -> EvalCtx {
        EvalCtx {
            user: super::super::eval::UserCtx {
                id:    Some(id.into()),
                role:  Some("user".into()),
                email: None,
                claims: Value::Null,
            },
            request_id: None,
        }
    }

    #[test]
    fn allow_deny() {
        let ctx = ctx_admin();
        let row = json!({});
        assert!(RuleExpr::Allow.evaluate(&ctx, &row, &Value::Null, &Value::Null));
        assert!(!RuleExpr::Deny.evaluate(&ctx, &row, &Value::Null, &Value::Null));
    }

    #[test]
    fn admin_role_check() {
        // ctx.user.role === 'admin'
        let rule = RuleExpr::Eq {
            left:  ValueExpr::CtxPath { path: vec!["user".into(), "role".into()] },
            right: ValueExpr::Str { value: "admin".into() },
        };
        assert!(rule.evaluate(&ctx_admin(), &json!({}), &Value::Null, &Value::Null));
        assert!(!rule.evaluate(&ctx_user("u2"), &json!({}), &Value::Null, &Value::Null));
    }

    #[test]
    fn owner_or_admin() {
        // ctx.user.id === row.id || ctx.user.role === 'admin'
        let rule = RuleExpr::Or { operands: vec![
            RuleExpr::Eq {
                left:  ValueExpr::CtxPath { path: vec!["user".into(), "id".into()] },
                right: ValueExpr::RowField { field: "id".into() },
            },
            RuleExpr::Eq {
                left:  ValueExpr::CtxPath { path: vec!["user".into(), "role".into()] },
                right: ValueExpr::Str { value: "admin".into() },
            },
        ]};

        let row = json!({ "id": "u1" });
        assert!(rule.evaluate(&ctx_user("u1"), &row, &Value::Null, &Value::Null));
        assert!(!rule.evaluate(&ctx_user("u99"), &row, &Value::Null, &Value::Null));
        assert!(rule.evaluate(&ctx_admin(), &row, &Value::Null, &Value::Null));
    }

    #[test]
    fn role_in_set() {
        // ["admin","mod"].includes(ctx.user.role)
        let rule = RuleExpr::In {
            value: ValueExpr::CtxPath { path: vec!["user".into(), "role".into()] },
            set:   vec![
                ValueExpr::Str { value: "admin".into() },
                ValueExpr::Str { value: "mod".into() },
            ],
        };
        assert!(rule.evaluate(&ctx_admin(), &json!({}), &Value::Null, &Value::Null));
        assert!(!rule.evaluate(&ctx_user("u1"), &json!({}), &Value::Null, &Value::Null));
    }

    #[test]
    fn roundtrip_json() {
        let rule = RuleExpr::And { operands: vec![
            RuleExpr::Eq {
                left:  ValueExpr::CtxPath { path: vec!["user".into(), "role".into()] },
                right: ValueExpr::Str { value: "admin".into() },
            },
            RuleExpr::Not { operand: Box::new(RuleExpr::Deny) },
        ]};
        let json = rule.to_json();
        let back = RuleExpr::from_json(&json).unwrap();
        // Re-evaluate to confirm roundtrip is correct
        assert!(back.evaluate(&ctx_admin(), &json!({}), &Value::Null, &Value::Null));
    }
}
