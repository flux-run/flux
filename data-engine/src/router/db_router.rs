use sqlx::PgPool;
use crate::engine::error::EngineError;

/// Routes a request to the correct PostgreSQL schema.
///
/// Flux is a single-project framework — the schema name is just the database
/// name (e.g. `"main"`).  No tenant prefix is applied.
pub struct DbRouter;

impl DbRouter {
    /// Validate and return the schema name for `db_name`.
    ///
    /// The schema name is the db_name itself (lowercased), validated as a safe
    /// Postgres identifier.
    pub fn schema_name(db_name: &str) -> Result<String, EngineError> {
        validate_identifier(db_name)?;
        Ok(db_name.to_lowercase())
    }

    /// CREATE the schema inside Postgres if it doesn't already exist.
    pub async fn create_schema(pool: &PgPool, schema: &str) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema))
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List all user-defined schema names (excludes Postgres system schemas).
    pub async fn list_schemas(pool: &PgPool) -> Result<Vec<String>, EngineError> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT nspname FROM pg_catalog.pg_namespace \
             WHERE nspname NOT IN ('pg_catalog','information_schema','flux_internal') \
               AND nspname NOT LIKE 'pg_%' \
             ORDER BY nspname",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// DROP the schema (CASCADE). Irreversible.
    pub async fn drop_schema(pool: &PgPool, schema: &str) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Verify a schema exists; return DatabaseNotFound if it doesn't.
    pub async fn assert_exists(pool: &PgPool, schema: &str) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        let exists: bool = sqlx::query_scalar(
            // Direct catalog lookup — O(1) index scan on pg_namespace.nspname regardless
            // of cluster size. information_schema.schemata includes visibility checks.
            "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1)",
        )
        .bind(schema)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Err(EngineError::DatabaseNotFound(schema.to_string()));
        }
        Ok(())
    }

    /// Verify a table exists within `schema`.
    pub async fn assert_table_exists(
        pool: &PgPool,
        schema: &str,
        table: &str,
    ) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        validate_identifier(table)?;

        let exists: bool = sqlx::query_scalar(
            // pg_catalog join is a direct index lookup (pg_namespace.nspname + pg_class.relname).
            // information_schema.tables evaluates ACL visibility for every row before filtering.
            // relkind = 'r' restricts to plain tables (excludes views, sequences, foreign tables).
            "SELECT EXISTS(\
               SELECT 1 FROM pg_catalog.pg_class c \
               JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
               WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = 'r')",
        )
        .bind(schema)
        .bind(table)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Err(EngineError::DatabaseNotFound(
                format!("table '{}' not found in schema '{}'", table, schema),
            ));
        }
        Ok(())
    }
}

/// Validate a PostgreSQL identifier: [a-zA-Z_][a-zA-Z0-9_]*, max 63 chars.
pub fn validate_identifier(s: &str) -> Result<(), EngineError> {
    if s.is_empty()
        || s.len() > 63
        || !s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        || !s.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Err(EngineError::InvalidIdentifier(s.to_string()));
    }
    Ok(())
}

/// Quote an identifier for safe inclusion in SQL.
pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
