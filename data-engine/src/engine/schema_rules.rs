//! Schema rule engine — evaluates compiled `RuleExpr` AST rules from `flux db push`.
//!
//! Rules are stored as JSONB in `fluxbase_internal.table_metadata.schema_rules`:
//! ```json
//! {
//!   "select": [RuleExpr, …],
//!   "insert": [RuleExpr, …],
//!   "update": [RuleExpr, …],
//!   "delete": [RuleExpr, …]
//! }
//! ```
//!
//! ## Evaluation semantics
//!
//! Rules for an operation are evaluated in order:
//! - `Deny { cond }` — if `cond` is true the request is immediately rejected (403).
//! - `Allow { cond }` — if `cond` is true the request is permitted.
//! - Any other top-level expr — treated as Allow if true.
//!
//! If **no rules** are registered for the operation → **allow** (open default).
//! If rules exist but **none allow** the request → **deny** (closed default).
//!
//! ## Limitation (v1)
//!
//! At this stage the engine is called before the SQL write, so `row` is
//! unavailable for UPDATE/DELETE. Rules that read `row.*` will receive
//! `Value::Null` for those fields. A pre-read can be added in a future pass.

use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    schema::{
        eval::{EvalCtx, UserCtx},
        rules::RuleExpr,
    },
};

pub struct SchemaRuleEngine;

impl SchemaRuleEngine {
    /// Enforce schema rules for `(table, operation)` against the request context.
    ///
    /// * `input` — the request payload (`req.data` for mutations, `Value::Null` for SELECT).
    /// * `row`   — the existing row value (available after pre-read, otherwise `Value::Null`).
    ///
    /// Returns `Ok(())` if access is granted, or `EngineError::AccessDenied` if denied.
    pub async fn enforce(
        pool: &PgPool,
        auth: &AuthContext,
        table: &str,
        operation: &str,
        input: &Value,
        row: &Value,
        request_id: &str,
    ) -> Result<(), EngineError> {
        let rules_json = Self::load(pool, table).await?;

        let Some(rules_json) = rules_json else {
            return Ok(()); // No schema rules registered → allow by default.
        };

        let op_rules = match rules_json.get(operation) {
            Some(v) if v.is_array() => v.as_array().unwrap().clone(),
            _ => return Ok(()), // No rules for this specific operation → allow.
        };

        if op_rules.is_empty() {
            return Ok(());
        }

        let ctx = Self::build_ctx(auth, request_id);
        let null = Value::Null;

        let mut any_rule_matched = false;

        for rule_val in &op_rules {
            let rule: RuleExpr = match serde_json::from_value(rule_val.clone()) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        table, operation,
                        error = %e,
                        "failed to deserialize RuleExpr — skipping"
                    );
                    continue;
                }
            };

            if rule.evaluate(&ctx, row, &null, input) {
                any_rule_matched = true;
                break; // OR semantics: first matching rule grants access
            }
        }

        if !any_rule_matched {
            tracing::warn!(
                table,
                operation,
                user_id = %auth.user_id,
                role    = %auth.role,
                "no schema rule matched — access denied"
            );
            return Err(EngineError::AccessDenied {
                role:      auth.role.clone(),
                table:     table.to_string(),
                operation: operation.to_string(),
            });
        }

        Ok(())
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn load(pool: &PgPool, table: &str) -> Result<Option<Value>, EngineError> {
        let row = sqlx::query(
            "SELECT schema_rules \
             FROM fluxbase_internal.table_metadata \
             WHERE table_name = $1 \
               AND schema_rules IS NOT NULL \
             LIMIT 1",
        )
        .bind(table)
        .fetch_optional(pool)
        .await
        .map_err(EngineError::Db)?;

        Ok(row.map(|r| r.get::<Value, _>("schema_rules")))
    }

    fn build_ctx(auth: &AuthContext, request_id: &str) -> EvalCtx {
        EvalCtx {
            user: UserCtx {
                id:    Some(auth.user_id.clone()),
                role:  Some(auth.role.clone()),
                email: None,
                claims: Value::Null,
            },
            request_id: Some(request_id.to_string()),
        }
    }
}
