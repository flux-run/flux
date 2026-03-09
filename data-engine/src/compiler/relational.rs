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

/// Load **all** relationships for a project schema in one query.
///
/// Callers pass the result into `CompilerOptions.relationships` so the compiler
/// can expand nested selectors at any depth without additional DB queries.
pub async fn load_all_relationships(
    pool: &PgPool,
    tenant_id: Uuid,
    project_id: Uuid,
    schema: &str,
) -> Result<Vec<RelationshipDef>, EngineError> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT alias, from_table, from_column, to_table, to_column, relationship \
         FROM fluxbase_internal.relationships \
         WHERE tenant_id = $1 AND project_id = $2 AND schema_name = $3",
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(schema)
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

// ─── Column selector ──────────────────────────────────────────────────────────

/// A recursively parsed column selector from the request's `columns` list.
///
/// ## Examples
/// | Input string | Result |
/// |---|---|
/// | `"id"` | `Flat("id")` |
/// | `"posts(*)"` | `Nested { alias="posts", cols=[] }` |
/// | `"posts(id,title)"` | `Nested { alias="posts", cols=[Flat("id"), Flat("title")] }` |
/// | `"posts(id,comments(id,body))"` | three-level nested tree |
#[derive(Debug, Clone)]
pub enum ColumnSelector {
    Flat(String),
    Nested { alias: String, cols: Vec<ColumnSelector> },
}

impl ColumnSelector {
    /// Produce a stable string fingerprint of the entire selector sub-tree.
    ///
    /// Used by the plan-cache key builder so that:
    /// - `posts(id,title)` and `posts(title,id)` map to the same plan.
    /// - `posts(id,comments(id,body))` is distinct from `posts(id)`.
    pub fn fingerprint(&self) -> String {
        match self {
            ColumnSelector::Flat(c) => c.clone(),
            ColumnSelector::Nested { alias, cols } => {
                let mut subs: Vec<String> = cols.iter().map(|c| c.fingerprint()).collect();
                subs.sort_unstable();
                format!("{}[{}]", alias, subs.join(","))
            }
        }
    }
}

/// Parse a slice of raw column strings (possibly containing nested selectors)
/// into a `Vec<ColumnSelector>`.
pub fn parse_selectors(columns: &[String]) -> Vec<ColumnSelector> {
    columns.iter().map(|c| parse_one(c.trim())).collect()
}

/// Parse a single selector string — handles arbitrary nesting depth.
fn parse_one(s: &str) -> ColumnSelector {
    if let Some(paren) = s.find('(') {
        let alias = s[..paren].trim().to_string();
        let inner = s[paren + 1..].trim_end_matches(')').trim();
        let cols = if inner == "*" || inner.is_empty() {
            vec![] // empty vec = "select all columns" for this relationship
        } else {
            // Paren-aware split so we don't break on commas inside sub-selectors.
            split_top_level(inner)
                .into_iter()
                .map(parse_one)
                .collect()
        };
        ColumnSelector::Nested { alias, cols }
    } else {
        ColumnSelector::Flat(s.to_string())
    }
}

/// Split `s` by commas that are NOT inside parentheses.
///
/// `"id,posts(id,title),name"` → `["id", "posts(id,title)", "name"]`
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

// ─── Deep lateral subquery expansion ─────────────────────────────────────────

/// Recursively expand a nested `ColumnSelector` into a Postgres lateral
/// subquery expression.
///
/// Uses the **derived-table pattern** (PostgREST approach) to avoid JOIN
/// explosions and support arbitrary column selection at each nesting level:
///
/// ```sql
/// -- users → posts(id,title) → comments(id,body)  [has_many → has_many]
/// (SELECT COALESCE(json_agg(__fb_posts), '[]'::json)
///  FROM (
///    SELECT
///      __fb_posts.id,
///      __fb_posts.title,
///      (SELECT COALESCE(json_agg(__fb_comments), '[]'::json)
///       FROM (SELECT __fb_comments.id, __fb_comments.body
///             FROM schema.comments __fb_comments
///             WHERE __fb_comments.post_id = __fb_posts.id) __fb_comments
///      ) AS "comments"
///    FROM schema.posts __fb_posts
///    WHERE __fb_posts.author_id = t.id
///  ) __fb_posts
/// ) AS "posts"
/// ```
///
/// Key properties:
/// - Each level uses a stable alias `__fb_<to_table>` — no name collisions.
/// - `all_rels` is the complete schema relationship registry; no further DB
///   queries are needed regardless of nesting depth.
/// - An unresolved nested selector emits `NULL AS "alias"` with a warning
///   rather than failing the whole query.
pub fn expand_nested_deep(
    schema: &str,
    outer_alias: &str,
    rel: &RelationshipDef,
    inner_sels: &[ColumnSelector],
    all_rels: &[RelationshipDef],
) -> String {
    // `__fb_<table>` avoids collisions with user column names and between nesting levels.
    let inner_alias = format!("__fb_{}", rel.to_table);

    // Build the SELECT column list for the derived table at this level.
    let col_list = if inner_sels.is_empty() {
        // No restriction — select all columns from the relationship target.
        format!("{}.*", inner_alias)
    } else {
        inner_sels
            .iter()
            .map(|sel| match sel {
                ColumnSelector::Flat(col) => {
                    format!("{}.{}", inner_alias, quote_ident(col))
                }
                ColumnSelector::Nested { alias, cols } => {
                    // Find this relationship in the all_rels registry,
                    // scoped to the current depth's table.
                    if let Some(child_rel) = all_rels.iter().find(|r| {
                        r.from_table == rel.to_table && &r.alias == alias
                    }) {
                        expand_nested_deep(schema, &inner_alias, child_rel, cols, all_rels)
                    } else {
                        tracing::warn!(
                            alias = %alias,
                            table = %rel.to_table,
                            "nested selector has no matching relationship — null substituted"
                        );
                        format!("NULL AS {}", quote_ident(alias))
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Wrap in appropriate aggregate based on relationship cardinality.
    // `json_agg(<derived_table_alias>)` aggregates *all columns* of each row
    // as a JSON object — this is valid standard Postgres syntax.
    if rel.is_array() {
        format!(
            "(SELECT COALESCE(json_agg({inner}), '[]'::json) \
             FROM (SELECT {col_list} FROM {schema}.{to_table} {inner} \
                   WHERE {inner}.{to_col} = {outer}.{from_col}) {inner}) AS {alias}",
            inner = inner_alias,
            col_list = col_list,
            schema = quote_ident(schema),
            to_table = quote_ident(&rel.to_table),
            outer = outer_alias,
            to_col = quote_ident(&rel.to_column),
            from_col = quote_ident(&rel.from_column),
            alias = quote_ident(&rel.alias),
        )
    } else {
        format!(
            "(SELECT row_to_json({inner}) \
             FROM (SELECT {col_list} FROM {schema}.{to_table} {inner} \
                   WHERE {inner}.{to_col} = {outer}.{from_col} LIMIT 1) {inner}) AS {alias}",
            inner = inner_alias,
            col_list = col_list,
            schema = quote_ident(schema),
            to_table = quote_ident(&rel.to_table),
            outer = outer_alias,
            to_col = quote_ident(&rel.to_column),
            from_col = quote_ident(&rel.from_column),
            alias = quote_ident(&rel.alias),
        )
    }
}
