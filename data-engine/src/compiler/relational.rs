use sqlx::PgPool;
use uuid::Uuid;

use crate::engine::error::EngineError;
use crate::router::db_router::quote_ident;

/// A relationship loaded from `fluxbase_internal.relationships`.
/// Passed into `CompilerOptions` so the compiler can expand nested column selectors.
#[derive(Debug, Clone)]
pub struct RelationshipDef {
    pub alias: String,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub relationship: String, // "has_one" | "has_many" | "belongs_to" | "many_to_many"
}

impl RelationshipDef {
    pub fn is_array(&self) -> bool {
        matches!(self.relationship.as_str(), "has_many" | "many_to_many")
    }
}

/// Load all relationships for a specific table in a project.
pub async fn load_relationships(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    schema: &str,
    table: &str,
) -> Result<Vec<RelationshipDef>, EngineError> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT alias, from_table, from_column, to_table, to_column, relationship \
         FROM fluxbase_internal.relationships \
         WHERE tenant_id = $1 AND project_id = $2 \
           AND schema_name = $3 AND from_table = $4",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(rows
        .iter()
        .map(|r| RelationshipDef {
            alias: r.get("alias"),
            from_table: r.get("from_table"),
            from_column: r.get("from_column"),
            to_table: r.get("to_table"),
            to_column: r.get("to_column"),
            relationship: r.get("relationship"),
        })
        .collect())
}

/// A parsed column selector from the request's `columns` list.
///
/// Input: `["id", "name", "posts(id,title)", "comments(*)"]`
/// Output: `[Flat("id"), Flat("name"), Nested{alias="posts", cols=["id","title"]}, Nested{alias="comments", cols=[]}]`
#[derive(Debug)]
pub enum ColumnSelector {
    Flat(String),
    Nested { alias: String, cols: Vec<String> },
}

pub fn parse_selectors(columns: &[String]) -> Vec<ColumnSelector> {
    columns.iter().map(|c| parse_one(c.trim())).collect()
}

fn parse_one(s: &str) -> ColumnSelector {
    if let Some(paren) = s.find('(') {
        let alias = s[..paren].trim().to_string();
        let inner = s[paren + 1..].trim_end_matches(')').trim();
        let cols: Vec<String> = if inner == "*" || inner.is_empty() {
            vec![]
        } else {
            inner.split(',').map(|c| c.trim().to_string()).collect()
        };
        ColumnSelector::Nested { alias, cols }
    } else {
        ColumnSelector::Flat(s.to_string())
    }
}

/// Expand a nested selector into a Postgres lateral subquery expression.
///
/// For has_many / many_to_many:
///   `(SELECT COALESCE(json_agg(row_to_json(r)), '[]') FROM schema.posts r
///     WHERE r.author_id = t.id) AS "posts"`
///
/// For has_one / belongs_to:
///   `(SELECT row_to_json(r) FROM schema.posts r
///     WHERE r.id = t.author_id LIMIT 1) AS "author"`
pub fn expand_nested(
    schema: &str,
    outer_table_alias: &str,
    rel: &RelationshipDef,
    inner_cols: &[String],
) -> String {
    let col_expr = if inner_cols.is_empty() {
        "row_to_json(r)".to_string()
    } else {
        let listed = inner_cols
            .iter()
            .map(|c| format!("r.{}", quote_ident(c)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("json_build_object({})", listed)
    };

    if rel.is_array() {
        format!(
            "(SELECT COALESCE(json_agg({col_expr}), '[]'::json) FROM {schema}.{to_table} r \
             WHERE r.{to_col} = {outer}.{from_col}) AS {alias}",
            col_expr = col_expr,
            schema = quote_ident(schema),
            to_table = quote_ident(&rel.to_table),
            outer = outer_table_alias,
            to_col = quote_ident(&rel.to_column),
            from_col = quote_ident(&rel.from_column),
            alias = quote_ident(&rel.alias),
        )
    } else {
        format!(
            "(SELECT {col_expr} FROM {schema}.{to_table} r \
             WHERE r.{to_col} = {outer}.{from_col} LIMIT 1) AS {alias}",
            col_expr = col_expr,
            schema = quote_ident(schema),
            to_table = quote_ident(&rel.to_table),
            outer = outer_table_alias,
            to_col = quote_ident(&rel.to_column),
            from_col = quote_ident(&rel.from_column),
            alias = quote_ident(&rel.alias),
        )
    }
}
