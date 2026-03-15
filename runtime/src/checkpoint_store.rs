//! Postgres-backed [`CheckpointStore`] implementation.
//!
//! Writes to `flux.checkpoints` and `flux.execution_records` using `sqlx`.
//! The pool is shared with the rest of the server binary.

use std::sync::Arc;
use uuid::Uuid;
use sqlx::PgPool;

use crate::checkpoint::{Checkpoint, BoundaryType, CheckpointStore};

// ── Postgres checkpoint store ─────────────────────────────────────────────────

pub struct PgCheckpointStore {
    pool: Arc<PgPool>,
}

impl PgCheckpointStore {
    pub fn new(pool: Arc<PgPool>) -> Arc<Self> {
        Arc::new(PgCheckpointStore { pool })
    }
}

#[async_trait::async_trait]
impl CheckpointStore for PgCheckpointStore {
    async fn write(&self, cp: &Checkpoint) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO flux.checkpoints \
             (id, execution_id, call_index, boundary, request, response, started_at_ms, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (execution_id, call_index) DO NOTHING",
        )
        .bind(cp.id)
        .bind(cp.execution_id)
        .bind(cp.call_index as i32)
        .bind(cp.boundary.as_str())
        .bind(&cp.request)
        .bind(&cp.response)
        .bind(cp.started_at_ms)
        .bind(cp.duration_ms as i32)
        .execute(&*self.pool)
        .await
        .map_err(|e| format!("checkpoint write failed: {}", e))?;
        Ok(())
    }

    async fn get(&self, execution_id: Uuid, call_index: u32) -> Result<Option<Checkpoint>, String> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id:            Uuid,
            execution_id:  Uuid,
            call_index:    i32,
            boundary:      String,
            request:       Vec<u8>,
            response:      Vec<u8>,
            started_at_ms: i64,
            duration_ms:   i32,
        }

        let row: Option<Row> = sqlx::query_as::<_, Row>(
            "SELECT id, execution_id, call_index, boundary, request, response, \
                    started_at_ms, duration_ms \
             FROM flux.checkpoints \
             WHERE execution_id = $1 AND call_index = $2",
        )
        .bind(execution_id)
        .bind(call_index as i32)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| format!("checkpoint fetch failed: {}", e))?;

        Ok(row.map(|r| Checkpoint {
            id:            r.id,
            execution_id:  r.execution_id,
            call_index:    r.call_index as u32,
            boundary:      if r.boundary == "db" { BoundaryType::Db } else { BoundaryType::Http },
            request:       r.request,
            response:      r.response,
            started_at_ms: r.started_at_ms,
            duration_ms:   r.duration_ms as u32,
        }))
    }
}

// ── Execution record helpers ──────────────────────────────────────────────────

/// Create a new execution record in Postgres with status='running'.
/// Returns the execution_id (UUID).
pub async fn create_execution_record(
    pool:        &PgPool,
    label:       &str,
    input:       Option<&serde_json::Value>,
    code_sha:    &str,
    instance_id: &str,
) -> Result<Uuid, String> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO flux.execution_records \
         (id, label, input, status, code_sha, instance_id) \
         VALUES ($1, $2, $3, 'running', $4, $5)",
    )
    .bind(id)
    .bind(label)
    .bind(input)
    .bind(code_sha)
    .bind(instance_id)
    .execute(pool)
    .await
    .map_err(|e| format!("create execution record failed: {}", e))?;
    Ok(id)
}

/// Finalise an execution record: set output, error, status, and duration_ms.
pub async fn finalize_execution_record(
    pool:        &PgPool,
    id:          Uuid,
    output:      Option<&serde_json::Value>,
    error:       Option<&str>,
    duration_ms: i32,
) -> Result<(), String> {
    let status = if error.is_some() { "error" } else { "ok" };
    sqlx::query(
        "UPDATE flux.execution_records \
         SET output = $1, error = $2, status = $3, duration_ms = $4 \
         WHERE id = $5",
    )
    .bind(output)
    .bind(error)
    .bind(status)
    .bind(duration_ms)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| format!("finalize execution record failed: {}", e))?;
    Ok(())
}

/// Fetch all checkpoints for an execution, ordered by call_index.
pub async fn list_checkpoints(
    pool:         &PgPool,
    execution_id: Uuid,
) -> Result<Vec<CheckpointSummary>, String> {
    #[derive(sqlx::FromRow)]
    struct Row {
        call_index:    i32,
        boundary:      String,
        request:       Vec<u8>,
        duration_ms:   i32,
        response:      Vec<u8>,
    }

    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT call_index, boundary, request, duration_ms, response \
         FROM flux.checkpoints \
         WHERE execution_id = $1 \
         ORDER BY call_index ASC",
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list checkpoints failed: {}", e))?;

    Ok(rows.into_iter().map(|r| {
        // Attempt to extract a human-readable label from the request bytes.
        let label = serde_json::from_slice::<serde_json::Value>(&r.request)
            .ok()
            .map(|v| {
                if r.boundary == "http" {
                    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("GET");
                    let url    = v.get("url").and_then(|u| u.as_str()).unwrap_or("?");
                    format!("{} {}", method, url)
                } else {
                    v.get("query").and_then(|q| q.as_str()).unwrap_or("?").to_string()
                }
            })
            .unwrap_or_default();

        let status_label = serde_json::from_slice::<serde_json::Value>(&r.response)
            .ok()
            .map(|v| {
                if r.boundary == "http" {
                    v.get("status")
                        .and_then(|s| s.as_u64())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                } else {
                    "ok".to_string()
                }
            })
            .unwrap_or_default();

        CheckpointSummary {
            call_index:  r.call_index as u32,
            boundary:    r.boundary,
            label,
            duration_ms: r.duration_ms as u32,
            status:      status_label,
        }
    }).collect())
}

#[derive(Debug, serde::Serialize)]
pub struct CheckpointSummary {
    pub call_index:  u32,
    pub boundary:    String,
    pub label:       String,
    pub duration_ms: u32,
    pub status:      String,
}
