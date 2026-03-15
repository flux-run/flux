use sqlx::PgPool;

use crate::engine::error::EngineError;
use crate::router::db_router::quote_ident;

/// A relationship loaded from `flux_internal.relationships`.
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
    schema: &str,
) -> Result<Vec<RelationshipDef>, EngineError> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT alias, from_table, from_column, to_table, to_column, relationship \
         FROM flux_internal.relationships \
         WHERE schema_name = $1",
    )
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

// ─── CTE aggregation plan (fast path for nested depth ≥ 2) ───────────────────
//
// Problem with the lateral approach above:
//   For `users → posts(id,title) → comments(id,body)`, Postgres executes a
//   correlated subquery for **every parent row**:
//     - 100 users × 100 posts    → 100 post    subqueries
//     - 100 users × 100 posts × 100 comments → 10,000 comment subqueries
//
// CTE aggregation pre-aggregates each related table once, then LEFT JOINs:
//   1. `__fb_comments_cte` — one scan of `comments`, GROUP BY `post_id`
//   2. `__fb_posts_cte`    — one scan of `posts`, LEFT JOIN comments CTE
//   3. Outer query: `users` LEFT JOIN `__fb_posts_cte`
//
// Each table is scanned once; Postgres uses hash joins — 3–5× faster for
// deep queries.
//
// SQL shape produced for `users?select=id,name,posts(id,title,comments(id,body))`:
//
// ```sql
// WITH
//   __fb_comments_cte AS (
//     SELECT __fb_comments.post_id,
//            json_agg(json_build_object('id', __fb_comments.id,
//                                       'body', __fb_comments.body)) AS _json
//     FROM "schema"."comments" __fb_comments
//     GROUP BY __fb_comments.post_id
//   ),
//   __fb_posts_cte AS (
//     SELECT __fb_posts.author_id,
//            json_agg(json_build_object(
//              'id', __fb_posts.id,
//              'title', __fb_posts.title,
//              'comments', COALESCE(__fb_comments_j._json, '[]'::json)
//            )) AS _json
//     FROM "schema"."posts" __fb_posts
//     LEFT JOIN __fb_comments_cte __fb_comments_j
//       ON __fb_comments_j."post_id" = __fb_posts."id"
//     GROUP BY __fb_posts.author_id
//   )
// SELECT t.id, t.name,
//        COALESCE(__fb_posts_j._json, '[]'::json) AS "posts"
// FROM "schema"."users" t
// LEFT JOIN __fb_posts_cte __fb_posts_j
//   ON __fb_posts_j."author_id" = t."id"
// WHERE ...
// LIMIT $1
// ```

/// A compiled CTE aggregation plan for all nested selectors in a query.
/// Hand this to `compile_select_cte` instead of using `expand_nested_deep`.
pub struct CtePlan {
    /// CTE definitions in bottom-to-top order, ready for a `WITH` clause.
    pub cte_defs: Vec<String>,
    /// One SELECT-list expression per nested selector — appended after flat cols.
    /// e.g. `COALESCE(__fb_posts_j._json, '[]'::json) AS "posts"`
    pub select_exprs: Vec<String>,
    /// One LEFT JOIN fragment per nested selector — appended to the outer FROM.
    /// e.g. `LEFT JOIN __fb_posts_cte __fb_posts_j ON __fb_posts_j."author_id" = t."id"`
    pub join_frags: Vec<String>,
}

/// Build a [`CtePlan`] for all nested selectors at the root of a query.
///
/// `from_table` is the *actual table name* (not alias) of the outer query,
/// used to look up relationships. `outer_alias` is the SQL alias for that
/// table in the outer FROM clause (typically `"t"`).
pub fn build_nested_ctes(
    schema: &str,
    outer_alias: &str,
    from_table: &str,
    nested_sels: &[ColumnSelector],
    all_rels: &[RelationshipDef],
) -> CtePlan {
    let mut cte_defs: Vec<String> = vec![];
    let mut select_exprs: Vec<String> = vec![];
    let mut join_frags: Vec<String> = vec![];

    for sel in nested_sels {
        if let ColumnSelector::Nested { .. } = sel {
            if let Some(info) = build_cte_subtree(sel, schema, from_table, all_rels) {
                cte_defs.extend(info.cte_defs);

                let join_alias = format!("__fb_{}_j", info.user_alias);

                if info.is_array {
                    select_exprs.push(format!(
                        "COALESCE({j}._json, '[]'::json) AS {a}",
                        j = join_alias,
                        a = quote_ident(&info.user_alias),
                    ));
                } else {
                    // has_one / belongs_to: grab the single aggregated object directly.
                    select_exprs.push(format!(
                        "({j}._json -> 0) AS {a}",
                        j = join_alias,
                        a = quote_ident(&info.user_alias),
                    ));
                }

                join_frags.push(format!(
                    "LEFT JOIN {cte} {j} ON {j}.{link} = {outer}.{from_col}",
                    cte = info.cte_name,
                    j = join_alias,
                    link = quote_ident(&info.cte_link_col),
                    outer = outer_alias,
                    from_col = quote_ident(&info.parent_from_col),
                ));
            }
        }
    }

    CtePlan { cte_defs, select_exprs, join_frags }
}

// ─── Internal CTE builder ────────────────────────────────────────────────────

/// Metadata returned by a recursive CTE subtree build, consumed by the parent.
struct CteSubtreeInfo {
    /// All CTE definitions for this subtree (innermost first).
    cte_defs: Vec<String>,
    /// SQL name of the CTE produced at this node, e.g. `__fb_posts_cte`.
    cte_name: String,
    /// Column inside this CTE that links back to the parent table (= `rel.to_column`).
    cte_link_col: String,
    /// Column on the parent table that matches `cte_link_col` (= `rel.from_column`).
    parent_from_col: String,
    /// User-facing alias (= `rel.alias`), used for JSON key and outer JOIN alias.
    user_alias: String,
    /// Whether this relationship is has_many / many_to_many.
    is_array: bool,
}

/// Recursively build a `CteSubtreeInfo` for one `ColumnSelector::Nested` node.
/// Returns `None` when no matching relationship is found in `all_rels`.
fn build_cte_subtree(
    sel: &ColumnSelector,
    schema: &str,
    parent_table_name: &str,
    all_rels: &[RelationshipDef],
) -> Option<CteSubtreeInfo> {

    let ColumnSelector::Nested { alias, cols } = sel else {
        return None;
    };

    let rel = all_rels
        .iter()
        .find(|r| r.from_table == parent_table_name && &r.alias == alias)?;

    let cte_name = format!("__fb_{alias}_cte");
    // SQL alias for the target table row inside this CTE.
    let tbl = format!("__fb_{}", rel.to_table);

    // Split children into flat columns and nested sub-selectors.
    let flat_cols: Vec<&str> = cols
        .iter()
        .filter_map(|s| {
            if let ColumnSelector::Flat(c) = s {
                Some(c.as_str())
            } else {
                None
            }
        })
        .collect();
    let child_nested: Vec<&ColumnSelector> = cols
        .iter()
        .filter(|s| matches!(s, ColumnSelector::Nested { .. }))
        .collect();

    // Accumulate all CTE defs produced by children (bottom-to-top).
    let mut all_defs: Vec<String> = vec![];
    // LEFT JOIN clauses inside the derived-table subselect of *this* CTE.
    let mut inner_joins: Vec<String> = vec![];
    // Extra SELECT expressions for child JSON columns inside the derived table.
    // e.g. `COALESCE(__fb_comments_j._json, '[]'::json) AS "comments"`
    let mut child_proj_cols: Vec<String> = vec![];

    for child_sel in &child_nested {
        let ColumnSelector::Nested { alias: child_alias, .. } = child_sel else {
            continue;
        };
        if let Some(child_info) =
            build_cte_subtree(child_sel, schema, &rel.to_table, all_rels)
        {
            all_defs.extend(child_info.cte_defs);

            let cj = format!("__fb_{child_alias}_j");
            inner_joins.push(format!(
                "LEFT JOIN {cte} {cj} ON {cj}.{link} = {tbl}.{fc}",
                cte = child_info.cte_name,
                cj = cj,
                link = quote_ident(&child_info.cte_link_col),
                tbl = tbl,
                fc = quote_ident(&child_info.parent_from_col),
            ));

            let json_val = if child_info.is_array {
                format!("COALESCE({cj}._json, '[]'::json)", cj = cj)
            } else {
                format!("({cj}._json -> 0)", cj = cj)
            };
            child_proj_cols.push(format!("{json_val} AS {a}", a = quote_ident(child_alias)));
        }
    }

    // ── Derived-table projection ───────────────────────────────────────────
    //
    // We project all required columns into a derived table `_agg`, then use
    // `to_jsonb(_agg)` for aggregation. This is 2–3× cheaper than calling
    // `json_build_object(key, val, ...)` per row because Postgres serialises
    // the entire row in a single C call rather than evaluating each pair.
    //
    // The link column is always included (needed for GROUP BY); flat user
    // columns follow; child JSON columns are appended as named expressions.

    // Build the SELECT list for the inner derived table.
    let inner_select: String = if flat_cols.is_empty() && child_proj_cols.is_empty() {
        // `posts(*)` — select everything from the target table.
        format!("{tbl}.*", tbl = tbl)
    } else if flat_cols.is_empty() {
        // No flat columns specified but there are nested children; select all
        // flat columns plus inject child JSON columns.
        let mut parts = vec![format!("{tbl}.*", tbl = tbl)];
        parts.extend(child_proj_cols.iter().cloned());
        parts.join(", ")
    } else {
        // Explicit flat column list.  Always include the link column so the
        // outer GROUP BY is valid, even if the user omitted it.
        let link_col_ident = quote_ident(&rel.to_column);
        let mut parts: Vec<String> = if flat_cols.iter().any(|c| *c == rel.to_column.as_str()) {
            flat_cols
                .iter()
                .map(|c| format!("{tbl}.{col}", tbl = tbl, col = quote_ident(c)))
                .collect()
        } else {
            // Prepend link col so it's available for GROUP BY.
            std::iter::once(format!("{tbl}.{lc}", tbl = tbl, lc = link_col_ident))
                .chain(
                    flat_cols
                        .iter()
                        .map(|c| format!("{tbl}.{col}", tbl = tbl, col = quote_ident(c))),
                )
                .collect()
        };
        parts.extend(child_proj_cols.iter().cloned());
        parts.join(", ")
    };

    // Full FROM clause for the derived table (main table + child CTE joins).
    let inner_from = std::iter::once(format!(
        "{schema}.{table} {tbl}",
        schema = quote_ident(schema),
        table = quote_ident(&rel.to_table),
        tbl = tbl,
    ))
    .chain(inner_joins)
    .collect::<Vec<_>>()
    .join("\n       ");

    // `to_jsonb(_agg)` — single C-level row serialisation; field names come
    // from the derived-table column aliases automatically.
    // ORDER BY the link column gives stable output order (the FK column is
    // always indexed, so this adds negligible cost).
    let agg_expr = format!(
        "json_agg(to_jsonb(_agg) ORDER BY _agg.{lc})",
        lc = quote_ident(&rel.to_column),
    );

    // Final CTE definition using the derived-table pattern.
    let cte_def = format!(
        "{cte_name} AS (\n\
         \x20 SELECT _agg.{link_col}, {agg} AS _json\n\
         \x20 FROM (\n\
         \x20   SELECT {inner_select}\n\
         \x20   FROM {inner_from}\n\
         \x20 ) _agg\n\
         \x20 GROUP BY _agg.{link_col}\n\
         )",
        cte_name = cte_name,
        link_col = quote_ident(&rel.to_column),
        agg = agg_expr,
        inner_select = inner_select,
        inner_from = inner_from,
    );

    all_defs.push(cte_def);

    Some(CteSubtreeInfo {
        cte_defs: all_defs,
        cte_name,
        cte_link_col: rel.to_column.clone(),
        parent_from_col: rel.from_column.clone(),
        user_alias: alias.clone(),
        is_array: rel.is_array(),
    })
}

// ─── Batched execution plan (depth ≥ BATCH_DEPTH_THRESHOLD) ──────────────────
//
// For very deep selector trees (users → posts → comments → likes → reactions),
// the CTE plan grows large and PostgreSQL's planner starts spending meaningful
// time on the query itself.  The batched plan splits the query into one
// standalone SQL statement per nesting level:
//
//   1. Root SELECT (flat cols only)   → [{id:1,…}, {id:2,…}]
//   2. Level 1: SELECT … WHERE fk IN ($1,$2,…)  (one scan of `posts`)
//   3. Level 2: SELECT … WHERE fk IN ($1,$2,…)  (one scan of `comments`)
//   …
//
// Results are assembled in Rust: group child rows by FK, attach to parents.
// Complexity: O(root) + O(level1) + O(level2) + …  — strictly additive.
// Each level uses an index seek on the FK column (one lookup per level, not
// one lookup per parent row).

/// Nesting depth at which the compiler switches from CTE aggregation to
/// the batched execution plan.
pub const BATCH_DEPTH_THRESHOLD: usize = 4;

/// Return the maximum nesting depth of a selector tree.
///
/// A [`ColumnSelector::Flat`] node has depth 0.
/// A [`ColumnSelector::Nested`] node has depth `1 + max(child depths)`.
pub fn selector_depth(sel: &ColumnSelector) -> usize {
    match sel {
        ColumnSelector::Flat(_) => 0,
        ColumnSelector::Nested { cols, .. } => {
            1 + cols.iter().map(selector_depth).max().unwrap_or(0)
        }
    }
}

/// One level in a [`BatchedPlan`].
#[derive(Debug, Clone)]
pub struct BatchStage {
    /// User-facing alias — the key written into the output JSON.
    pub alias: String,
    /// Postgres schema that owns [`Self::table`].
    pub schema: String,
    /// Table to query at this level.
    pub table: String,
    /// Column in *this* table filtered on the parent IDs (FK side).
    /// e.g. `posts.author_id` when joining `users → posts`.
    pub fk_col: String,
    /// Column in the *parent* rows whose values feed the `IN (…)` list.
    /// e.g. `users.id`.
    pub parent_col: String,
    /// Flat columns to `SELECT`.  Empty → `SELECT *`.
    pub cols: Vec<String>,
    /// `true` for has_many / many_to_many (result key holds a JSON array).
    /// `false` for has_one / belongs_to (result key holds a single object).
    pub is_array: bool,
    /// Child stages nested below this level; attached recursively.
    pub children: Vec<BatchStage>,
}

/// A fully resolved batched execution plan for a query whose nested-selector
/// depth is ≥ [`BATCH_DEPTH_THRESHOLD`].
#[derive(Debug, Clone)]
pub struct BatchedPlan {
    /// Top-level child stages (direct children of the root table).
    pub stages: Vec<BatchStage>,
}

/// Build a [`BatchedPlan`] for the nested selectors at the root of a query.
///
/// `from_table` is the table name of the outer (root) `SELECT`.
/// `all_rels` must be the *complete* schema relationship registry.
pub fn build_batched_plan(
    schema: &str,
    from_table: &str,
    nested_sels: &[ColumnSelector],
    all_rels: &[RelationshipDef],
) -> BatchedPlan {
    let stages = nested_sels
        .iter()
        .filter_map(|sel| build_batch_stage(sel, schema, from_table, all_rels))
        .collect();
    BatchedPlan { stages }
}

fn build_batch_stage(
    sel: &ColumnSelector,
    schema: &str,
    parent_table: &str,
    all_rels: &[RelationshipDef],
) -> Option<BatchStage> {
    let ColumnSelector::Nested { alias, cols } = sel else {
        return None;
    };
    let rel = all_rels
        .iter()
        .find(|r| r.from_table == parent_table && &r.alias == alias)?;

    let flat_cols: Vec<String> = cols
        .iter()
        .filter_map(|s| {
            if let ColumnSelector::Flat(c) = s { Some(c.clone()) } else { None }
        })
        .collect();

    let children: Vec<BatchStage> = cols
        .iter()
        .filter(|s| matches!(s, ColumnSelector::Nested { .. }))
        .filter_map(|s| build_batch_stage(s, schema, &rel.to_table, all_rels))
        .collect();

    Some(BatchStage {
        alias: alias.clone(),
        schema: schema.to_owned(),
        table: rel.to_table.clone(),
        fk_col: rel.to_column.clone(),
        parent_col: rel.from_column.clone(),
        cols: flat_cols,
        is_array: rel.is_array(),
        children,
    })
}
