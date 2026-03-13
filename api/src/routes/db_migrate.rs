//! `POST /internal/db/migrate` — apply a single user-owned SQL migration.
//!
//! This endpoint is called by `flux db push` for each migration file.
//! It is idempotent: if the migration has already been applied it returns
//! `{ "status": "already_applied" }` immediately.
//!
//! Migrations are tracked in `flux.user_migrations`. This table is separate
//! from the Flux system migration tables in `schema_migrations` (sqlx) and
//! `flux_migrations` (internal), so user schema changes are isolated.
//!
//! ## SOLID
//!
//! - SRP: This handler only applies one migration at a time.  Ordering and
//!   collection are the CLI's responsibility.
//! - OCP: The tracking strategy (table lookup) can change without affecting
//!   callers; the request/response contract stays stable.

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::AppState;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MigrateRequest {
    /// The filename, e.g. `001_create_users.sql`. Used as the unique key.
    pub name: String,
    /// The full SQL content of the migration file.
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MigrateResponse {
    /// `"applied"` or `"already_applied"`
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

pub async fn apply_user_migration(
    State(state): State<AppState>,
    Json(req): Json<MigrateRequest>,
) -> Result<Json<MigrateResponse>, (StatusCode, String)> {
    let pool = &state.pool;

    // 1. Ensure tracking table exists.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS flux.user_migrations (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT        NOT NULL UNIQUE,
            applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 2. Check if already applied.
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM flux.user_migrations WHERE name = $1)"
    )
    .bind(&req.name)
    .fetch_one(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if row.0 {
        return Ok(Json(MigrateResponse {
            status:  "already_applied".into(),
            message: None,
        }));
    }

    // 3. Run inside a transaction so partial failures are rolled back.
    let mut tx = pool
        .begin()
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query(&req.content)
        .execute(&mut *tx)
        .await
        .map_err(|e: sqlx::Error| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Migration '{}' failed: {}", req.name, e),
            )
        })?;

    sqlx::query("INSERT INTO flux.user_migrations (name) VALUES ($1)")
        .bind(&req.name)
        .execute(&mut *tx)
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(migration = %req.name, "user migration applied");

    Ok(Json(MigrateResponse {
        status:  "applied".into(),
        message: None,
    }))
}
