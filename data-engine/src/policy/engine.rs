use std::collections::HashMap;
use sqlx::{FromRow, PgPool};
use tokio::sync::RwLock;
use uuid::Uuid;
use crate::engine::auth_context::AuthContext;
use crate::engine::error::EngineError;

/// In-process policy cache.
/// Key: "<tenant_id>:<project_id>:<table>:<role>:<operation>"
pub type PolicyCache = RwLock<HashMap<String, PolicyResult>>;

/// A row from `fluxbase_internal.policies`.
#[derive(FromRow, Clone)]
struct PolicyRow {
    allowed_columns: serde_json::Value,
    row_condition: Option<String>,
}

/// Result of policy evaluation for a given (role, table, operation) triple.
#[derive(Clone, Debug)]
pub struct PolicyResult {
    /// Columns the role may see. Empty vec = all columns permitted (SELECT *).
    pub allowed_columns: Vec<String>,
    /// Pre-processed row-level condition with `$auth.*` vars substituted to `$N`.
    pub row_condition_sql: Option<String>,
    /// Bind values for `row_condition_sql`, in parameter order.
    pub row_condition_params: Vec<serde_json::Value>,
}

pub struct PolicyEngine;

impl PolicyEngine {
    /// Build the cache key for a given (tenant, project, table, role, operation).
    pub fn cache_key(
        tenant_id: Uuid,
        project_id: Uuid,
        table: &str,
        role: &str,
        operation: &str,
    ) -> String {
        format!("{}:{}:{}:{}:{}", tenant_id, project_id, table, role, operation)
    }

    /// Evaluate policy, using `cache` as a read-through in-process cache.
    pub async fn evaluate_cached(
        pool: &PgPool,
        auth: &AuthContext,
        table: &str,
        operation: &str,
        cache: &PolicyCache,
    ) -> Result<PolicyResult, EngineError> {
        let key = Self::cache_key(auth.tenant_id, auth.project_id, table, &auth.role, operation);

        // Fast path — read lock only.
        {
            let guard = cache.read().await;
            if let Some(hit) = guard.get(&key) {
                tracing::debug!(key = %key, "policy cache hit");
                return Ok(hit.clone());
            }
        }

        // Slow path — load from DB, then write to cache.
        let result = Self::evaluate(pool, auth, table, operation).await?;
        {
            let mut guard = cache.write().await;
            guard.insert(key, result.clone());
        }
        Ok(result)
    }

    /// Load and evaluate the policy for (`role`, `table`, `operation`).
    ///
    /// Evaluation order:
    ///   1. Exact match: (role, table, operation)
    ///   2. Wildcard operation: (role, table, '*')
    ///
    /// Returns `AccessDenied` if neither match exists.
    pub async fn evaluate(
        pool: &PgPool,
        auth: &AuthContext,
        table: &str,
        operation: &str,
    ) -> Result<PolicyResult, EngineError> {
        let row = load_policy(pool, auth.tenant_id, auth.project_id, &auth.role, table, operation)
            .await?;

        let allowed_columns = parse_columns(&row.allowed_columns);
        let (row_condition_sql, row_condition_params) = substitute_condition(
            row.row_condition.as_deref(),
            auth,
            1, // param index starts at 1
        );

        Ok(PolicyResult {
            allowed_columns,
            row_condition_sql,
            row_condition_params,
        })
    }
}

async fn load_policy(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    role: &str,
    table: &str,
    operation: &str,
) -> Result<PolicyRow, EngineError> {
    // Try exact operation match first, then wildcard '*'.
    let row = sqlx::query_as::<_, PolicyRow>(
        "SELECT allowed_columns, row_condition
         FROM fluxbase_internal.policies
         WHERE tenant_id = $1
           AND project_id = $2
           AND table_name = $3
           AND role = $4
           AND operation = ANY(ARRAY[$5, '*'])
         ORDER BY CASE WHEN operation = $5 THEN 0 ELSE 1 END
         LIMIT 1",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(table)
    .bind(role)
    .bind(operation)
    .fetch_optional(pool)
    .await?;

    row.ok_or_else(|| EngineError::AccessDenied {
        role: role.to_string(),
        table: table.to_string(),
        operation: operation.to_string(),
    })
}

/// Parse `allowed_columns` from JSON array to `Vec<String>`.
/// An empty array means all columns are allowed.
fn parse_columns(val: &serde_json::Value) -> Vec<String> {
    match val.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => vec![],
    }
}

/// Replace `$auth.*` template variables in a row_condition with `$N` placeholders.
///
/// Returns the rewritten SQL fragment and the corresponding bind values.
/// `param_start` is the index for the first new placeholder (1-based).
fn substitute_condition(
    template: Option<&str>,
    auth: &AuthContext,
    param_start: usize,
) -> (Option<String>, Vec<serde_json::Value>) {
    let Some(tmpl) = template else {
        return (None, vec![]);
    };

    let mut sql = tmpl.to_string();
    let mut params: Vec<serde_json::Value> = vec![];
    let mut idx = param_start;

    let substitutions: &[(&str, serde_json::Value)] = &[
        ("$auth.uid", serde_json::Value::String(auth.user_id.clone())),
        ("$auth.role", serde_json::Value::String(auth.role.clone())),
        (
            "$auth.tenant_id",
            serde_json::Value::String(auth.tenant_id.to_string()),
        ),
        (
            "$auth.project_id",
            serde_json::Value::String(auth.project_id.to_string()),
        ),
    ];

    for (var, val) in substitutions {
        if sql.contains(var) {
            sql = sql.replace(var, &format!("${}", idx));
            params.push(val.clone());
            idx += 1;
        }
    }

    (Some(sql), params)
}
