use sqlx::PgPool;
use uuid::Uuid;

use crate::engine::{auth_context::AuthContext, error::EngineError};

pub struct EventEmitter;

impl EventEmitter {
    /// Emit a data event to `fluxbase_internal.events`.
    ///
    /// Event type convention: `"{table}.{verb}"` — e.g. `"users.inserted"`,
    /// `"orders.updated"`, `"sessions.deleted"`.
    ///
    /// Errors are deliberately logged and swallowed rather than surfaced to the
    /// caller — event emission must never cause a user-facing request to fail.
    pub async fn emit(
        pool: &PgPool,
        auth: &AuthContext,
        table: &str,
        verb: &str,
        payload: &serde_json::Value,
    ) {
        let event_type = format!("{}.{}", table, verb);
        let result = sqlx::query(
            "INSERT INTO fluxbase_internal.events \
                 (tenant_id, project_id, event_type, table_name, payload) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(auth.tenant_id)
        .bind(auth.project_id)
        .bind(&event_type)
        .bind(table)
        .bind(payload)
        .execute(pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(
                event_type = %event_type,
                error = %e,
                "failed to emit event (non-fatal)"
            );
        } else {
            tracing::debug!(event_type = %event_type, "event emitted");
        }
    }

    /// Convenience: map SQL operation name to event verb.
    pub fn verb_for(operation: &str) -> Option<&'static str> {
        match operation {
            "insert" => Some("inserted"),
            "update" => Some("updated"),
            "delete" => Some("deleted"),
            _ => None, // SELECT does not emit an event
        }
    }

    /// Fetch recent undelivered events for this tenant+project.
    /// Intended for the event monitoring API, not the hot path.
    pub async fn recent(
        pool: &PgPool,
        tenant_id: Uuid,
        project_id: Uuid,
        since_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        let rows = if let Some(cursor) = since_id {
            sqlx::query(
                "SELECT id, event_type, table_name, payload, created_at \
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
                "SELECT id, event_type, table_name, payload, created_at \
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

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":         r.get::<Uuid, _>("id"),
                    "event_type": r.get::<String, _>("event_type"),
                    "table_name": r.get::<String, _>("table_name"),
                    "payload":    r.get::<serde_json::Value, _>("payload"),
                })
            })
            .collect())
    }
}
