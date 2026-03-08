use sqlx::PgPool;
use crate::engine::error::EngineError;

/// Computes the PostgreSQL schema name for a project database.
///
/// Convention:  t_{tenant_slug}_{project_slug}_{db_name}
/// Example:     t_acme_auth_main
///
/// All slugs are lowercased and hyphens are converted to underscores by the
/// auth context extractor, so this function only needs to assemble the parts.
pub struct DbRouter;

impl DbRouter {
    /// Compute schema name from context parts. Returns Err if any part fails
    /// identifier validation.
    pub fn schema_name(
        tenant_slug: &str,
        project_slug: &str,
        db_name: &str,
    ) -> Result<String, EngineError> {
        for part in [tenant_slug, project_slug, db_name] {
            validate_identifier(part)?;
        }
        Ok(format!(
            "t_{}_{}_{}", 
            tenant_slug.to_lowercase(),
            project_slug.to_lowercase(),
            db_name.to_lowercase(),
        ))
    }

    /// CREATE the schema inside Postgres if it doesn't already exist.
    pub async fn create_schema(pool: &PgPool, schema: &str) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema))
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List all schema names owned by this tenant+project pair.
    pub async fn list_schemas(
        pool: &PgPool,
        tenant_slug: &str,
        project_slug: &str,
    ) -> Result<Vec<String>, EngineError> {
        let prefix = format!(
            "t_{}_{}_%",
            tenant_slug.to_lowercase(),
            project_slug.to_lowercase()
        );
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name LIKE $1 ORDER BY schema_name",
        )
        .bind(&prefix)
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
            "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
        )
        .bind(schema)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Err(EngineError::DatabaseNotFound(schema.to_string()));
        }
        Ok(())
    }

    /// Verify a table exists within `schema` and reject system catalog access.
    ///
    /// User-owned schemas always start with `t_`; anything else is blocked as a
    /// defence-in-depth measure even if identifier validation already passed.
    pub async fn assert_table_exists(
        pool: &PgPool,
        schema: &str,
        table: &str,
    ) -> Result<(), EngineError> {
        validate_identifier(schema)?;
        validate_identifier(table)?;

        if !schema.starts_with("t_") {
            return Err(EngineError::AccessDenied {
                role: "any".into(),
                table: table.into(),
                operation: "any".into(),
            });
        }

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
               SELECT 1 FROM information_schema.tables \
               WHERE table_schema = $1 AND table_name = $2 AND table_type = 'BASE TABLE')",
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
