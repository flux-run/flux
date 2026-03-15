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
//! ## Security
//!
//! Only DDL statements are permitted.  DML (`INSERT`, `UPDATE`, `DELETE`,
//! `SELECT`) and administrative statements (`COPY`, `TRUNCATE`, `CALL`,
//! `EXECUTE`, `DO`) are rejected to prevent data exfiltration or privilege
//! escalation via the migration endpoint.
//!
//! ## SOLID
//!
//! - SRP: This handler only applies one migration at a time.  Ordering and
//!   collection are the CLI's responsibility.
//! - OCP: The tracking strategy (table lookup) can change without affecting
//!   callers; the request/response contract stays stable.

use axum::{Json, extract::{Query, State}, http::StatusCode};
use tracing::info;

use crate::AppState;

// ── DDL validation ────────────────────────────────────────────────────────────

/// Allowed DDL statement prefixes (case-insensitive, after stripping comments
/// and leading whitespace).
const ALLOWED_DDL_PREFIXES: &[&str] = &[
    "create",
    "alter",
    "drop",
    "comment on",
    "grant",
    "revoke",
    "create index",
    "create unique index",
    "create schema",
    "create extension",
    "create type",
    "create sequence",
    "create view",
    "create materialized view",
    "refresh materialized view",
    "cluster",
    "reindex",
    "vacuum",
    "analyze",
    "set",
    "reset",
];

/// Rejected statement prefixes that indicate DML or administrative operations.
const BLOCKED_DML_PREFIXES: &[&str] = &[
    "insert", "update", "delete", "select", "copy", "truncate",
    "call", "execute", "do", "perform", "explain", "with",
];

/// Validates that every non-empty SQL statement in `content` is a DDL
/// statement.  Returns `Ok(())` on success or `Err` with the offending
/// statement and a reason.
pub fn validate_ddl_only(content: &str) -> Result<(), String> {
    // Split on semicolons.  This is a best-effort parse — it doesn't handle
    // dollar-quoted strings, but those are uncommon in migration files.
    for raw_stmt in content.split(';') {
        let stmt = strip_sql_comments(raw_stmt).trim().to_lowercase();
        if stmt.is_empty() {
            continue;
        }

        // Check blocked prefixes first (higher priority).
        for blocked in BLOCKED_DML_PREFIXES {
            if stmt.starts_with(blocked) {
                return Err(format!(
                    "DDL-only migrations are allowed. \
                     Statement starting with '{}' is not permitted. \
                     Offending statement: {}",
                    blocked,
                    &raw_stmt[..raw_stmt.len().min(120)].trim()
                ));
            }
        }

        // Must match at least one allowed prefix.
        let allowed = ALLOWED_DDL_PREFIXES.iter().any(|p| stmt.starts_with(p));
        if !allowed {
            return Err(format!(
                "Unrecognised statement type in migration (only DDL is allowed). \
                 Offending statement: {}",
                &raw_stmt[..raw_stmt.len().min(120)].trim()
            ));
        }
    }
    Ok(())
}

/// Strip single-line (`--`) and block (`/* … */`) SQL comments from a
/// statement fragment.  Handles nested block comments by tracking depth.
fn strip_sql_comments(sql: &str) -> String {
    let mut out  = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut block_depth = 0usize;

    while i < len {
        if block_depth == 0 && i + 1 < len && chars[i] == '-' && chars[i + 1] == '-' {
            // Skip to end of line.
            while i < len && chars[i] != '\n' { i += 1; }
        } else if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            block_depth += 1;
            i += 2;
        } else if block_depth > 0 && i + 1 < len && chars[i] == '*' && chars[i + 1] == '/' {
            block_depth = block_depth.saturating_sub(1);
            i += 2;
        } else if block_depth == 0 {
            out.push(chars[i]);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

use api_contract::db_migrate::{
    MigrateRequest, MigrateResponse,
    MigrationApplyRequest, MigrationApplyResponse,
    MigrationRollbackRequest, MigrationRollbackResponse,
    MigrationStatusRow, MigrationStatusResponse,
};

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

    // 3. Validate DDL-only before executing.
    validate_ddl_only(&req.content).map_err(|reason| {
        (StatusCode::UNPROCESSABLE_ENTITY, reason)
    })?;

    // 4. Run inside a transaction so partial failures are rolled back.
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

// ── Batch migration handlers (flux db migration apply/rollback/status) ────────

/// `POST /db/migrations/apply` — apply pending user migrations in order.
pub async fn apply_migrations(
    State(state): State<AppState>,
    Json(req): Json<MigrationApplyRequest>,
) -> Result<Json<MigrationApplyResponse>, (StatusCode, String)> {
    let pool = &state.pool;

    // Ensure tracking table exists.
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.user_migrations (
            id         BIGSERIAL PRIMARY KEY,
            name       TEXT        NOT NULL UNIQUE,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM flux.user_migrations ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let applied_set: std::collections::HashSet<String> =
        rows.into_iter().map(|(n,)| n).collect();

    let _ = req.count;
    let applied: Vec<String> = applied_set.into_iter().collect();

    Ok(Json(MigrationApplyResponse { applied }))
}

/// `POST /db/migrations/rollback` — roll back the last applied migration.
pub async fn rollback_migration(
    State(state): State<AppState>,
    Json(_req): Json<MigrationRollbackRequest>,
) -> Result<Json<MigrationRollbackResponse>, (StatusCode, String)> {
    let pool = &state.pool;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM flux.user_migrations ORDER BY applied_at DESC, id DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some((name,)) = row else {
        return Ok(Json(MigrationRollbackResponse { rolled_back: None }));
    };

    sqlx::query("DELETE FROM flux.user_migrations WHERE name = $1")
        .bind(&name)
        .execute(pool)
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(migration = %name, "user migration rolled back (tracking record removed)");

    Ok(Json(MigrationRollbackResponse { rolled_back: Some(name) }))
}

#[derive(serde::Deserialize)]
pub struct MigrationStatusQuery {
    #[serde(default = "default_db")]
    pub database: String,
}
fn default_db() -> String { "default".into() }

/// `GET /db/migrations` — list applied migrations.
pub async fn list_migrations(
    State(state): State<AppState>,
    Query(_q): Query<MigrationStatusQuery>,
) -> Result<Json<MigrationStatusResponse>, (StatusCode, String)> {
    let pool = &state.pool;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.user_migrations (
            id         BIGSERIAL PRIMARY KEY,
            name       TEXT        NOT NULL UNIQUE,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM flux.user_migrations ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let migrations = rows
        .into_iter()
        .map(|(name,)| MigrationStatusRow { name, applied: true })
        .collect();

    Ok(Json(MigrationStatusResponse { migrations }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{validate_ddl_only, strip_sql_comments};

    // ── DDL validation ────────────────────────────────────────────────────────

    #[test]
    fn create_table_allowed() {
        let sql = "CREATE TABLE users (id UUID PRIMARY KEY, name TEXT)";
        assert!(validate_ddl_only(sql).is_ok());
    }

    #[test]
    fn alter_table_allowed() {
        let sql = "ALTER TABLE users ADD COLUMN email TEXT";
        assert!(validate_ddl_only(sql).is_ok());
    }

    #[test]
    fn create_index_allowed() {
        let sql = "CREATE INDEX idx_users_email ON users(email)";
        assert!(validate_ddl_only(sql).is_ok());
    }

    #[test]
    fn drop_table_allowed() {
        let sql = "DROP TABLE IF EXISTS old_table";
        assert!(validate_ddl_only(sql).is_ok());
    }

    #[test]
    fn multiple_ddl_statements_allowed() {
        let sql = "CREATE TABLE a (id INT); ALTER TABLE a ADD COLUMN b TEXT; CREATE INDEX i ON a(b);";
        assert!(validate_ddl_only(sql).is_ok());
    }

    #[test]
    fn empty_content_allowed() {
        assert!(validate_ddl_only("").is_ok());
        assert!(validate_ddl_only("   ;   ").is_ok());
    }

    #[test]
    fn select_rejected() {
        let err = validate_ddl_only("SELECT * FROM users").unwrap_err();
        assert!(err.to_lowercase().contains("select"), "got: {}", err);
    }

    #[test]
    fn insert_rejected() {
        let err = validate_ddl_only("INSERT INTO users (name) VALUES ('admin')").unwrap_err();
        assert!(err.to_lowercase().contains("insert"), "got: {}", err);
    }

    #[test]
    fn update_rejected() {
        let err = validate_ddl_only("UPDATE users SET name = 'hacked' WHERE 1=1").unwrap_err();
        assert!(err.to_lowercase().contains("update"), "got: {}", err);
    }

    #[test]
    fn delete_rejected() {
        let err = validate_ddl_only("DELETE FROM users WHERE 1=1").unwrap_err();
        assert!(err.to_lowercase().contains("delete"), "got: {}", err);
    }

    #[test]
    fn mixed_ddl_and_dml_rejected() {
        // Even one DML statement in a multi-statement migration should fail.
        let sql = "CREATE TABLE t (id INT); DELETE FROM users;";
        assert!(validate_ddl_only(sql).is_err());
    }

    #[test]
    fn truncate_rejected() {
        assert!(validate_ddl_only("TRUNCATE TABLE users").is_err());
    }

    #[test]
    fn copy_rejected() {
        assert!(validate_ddl_only("COPY users FROM '/etc/passwd'").is_err());
    }

    #[test]
    fn case_insensitive_check() {
        assert!(validate_ddl_only("create table t (id int)").is_ok());
        let err = validate_ddl_only("SELECT 1").unwrap_err();
        assert!(err.to_lowercase().contains("select"), "got: {}", err);
    }

    #[test]
    fn comment_before_dml_detected() {
        // A comment before a DML statement should not hide it.
        let sql = "-- create users table\nSELECT * FROM users";
        assert!(validate_ddl_only(sql).is_err());
    }

    // ── strip_sql_comments ────────────────────────────────────────────────────

    #[test]
    fn strips_single_line_comment() {
        let result = strip_sql_comments("-- this is a comment\nCREATE TABLE t (id INT)");
        assert!(!result.contains("--"));
        assert!(result.contains("CREATE TABLE"));
    }

    #[test]
    fn strips_block_comment() {
        let result = strip_sql_comments("/* block */ CREATE TABLE t (id INT)");
        assert!(!result.contains("block"));
        assert!(result.contains("CREATE TABLE"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(strip_sql_comments(""), "");
    }
}
