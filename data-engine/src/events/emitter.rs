use sqlx::PgPool;
use uuid::Uuid;

use crate::engine::{auth_context::AuthContext, error::EngineError};

pub struct EventEmitter;

impl EventEmitter {
    /// Emit a data event to `fluxbase_internal.events`.
    ///
    /// - `operation` — "insert" | "update" | "delete"
    /// - `record_id`  — PK of the mutated row (as text); None when not available.
    /// - `payload`    — full RETURNING row(s) from the executor.
    ///
    /// Errors are logged and swallowed — emission must never fail a user request.
    pub async fn emit(
        pool: &PgPool,
        auth: &AuthContext,
        table: &str,
        operation: &str,
        record_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let event_type = format!("{}.{}d", table, operation); // inserted / updated / deleted
        let result = sqlx::query(
            "INSERT INTO fluxbase_internal.events \
                 (tenant_id, project_id, event_type, table_name, \
                  record_id, operation, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(auth.tenant_id)
        .bind(auth.project_id)
        .bind(&event_type)
        .bind(table)
        .bind(record_id)
        .bind(operation)
        .bind(payload)
        .execute(pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(event_type = %event_type, error = %e, "failed to emit event (non-fatal)");
        } else {
            tracing::debug!(event_type = %event_type, "event emitted");
        }
    }

    /// Convenience: return the DML verb for use as the `operation` arg.
    /// Returns `None` for SELECT (no event emitted).
    pub fn verb_for(operation: &str) -> Option<&'static str> {
        match operation {
            "insert" => Some("insert"),
            "update" => Some("update"),
            "delete" => Some("delete"),
            _ => None,
        }
    }

    /// Extract the `id` field from a RETURNING row as a string, if present.
    /// Used to populate `record_id` without assuming a specific PK column name.
    pub fn extract_record_id(result: &serde_json::Value) -> Option<String> {
        // Result may be an array (multi-row RETURNING) or a single object.
        let first = match result {
            serde_json::Value::Array(arr) => arr.first()?,
            obj @ serde_json::Value::Object(_) => obj,
            _ => return None,
        };
        first.get("id").and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    }

    /// Fetch undelivered events for this tenant+project (cursor-based).
    pub async fn recent(
        pool: &PgPool,
        tenant_id: Uuid,
        project_id: Uuid,
        since_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        use sqlx::Row;
        let rows = if let Some(cursor) = since_id {
            sqlx::query(
                "SELECT id, event_type, table_name, record_id, operation, payload, created_at \
                 FROM fluxbase_internal.events \
                 WHERE tenant_id = $1 AND project_id = $2 \
                   AND created_at > (SELECT created_at FROM fluxbase_internal.events WHERE id = $3) \
                 ORDER BY created_at \
                 LIMIT $4",
            )
            .bind(tenant_id)
            .bind(project_id)
            .bind(cursor)
            .bind(limit)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, event_type, table_name, record_id, operation, payload, created_at \
                 FROM fluxbase_internal.events \
                 WHERE tenant_id = $1 AND project_id = $2 \
                 ORDER BY created_at DESC \
                 LIMIT $3",
            )
            .bind(tenant_id)
            .bind(project_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        .map_err(EngineError::Db)?;

        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":         r.get::<Uuid, _>("id"),
                    "event_type": r.get::<String, _>("event_type"),
                    "table_name": r.get::<String, _>("table_name"),
                    "record_id":  r.get::<Option<String>, _>("record_id"),
                    "operation":  r.get::<String, _>("operation"),
                    "payload":    r.get::<serde_json::Value, _>("payload"),
                })
            })
            .collect())
    }
}

