//! Tool schema builder — converts function definitions into LLM tool_use format.
//!
//! Reads `name`, `description`, and `input_schema` from the `functions` table
//! and produces the OpenAI tool schema array required in the LLM request.
//!
//! Output format per tool:
//! ```json
//! {
//!   "type": "function",
//!   "function": {
//!     "name": "create_issue",
//!     "description": "Creates a GitHub issue in the configured repository.",
//!     "parameters": { <JSON Schema from input_schema column> }
//!   }
//! }
//! ```

use sqlx::PgPool;

/// Build the tools array for the LLM request from the function registry.
/// Functions that are missing from the DB are silently skipped (agent may still
/// list them; registry.deploy_from_yaml already validated they exist).
pub async fn build_tool_schemas(
    pool:       &PgPool,
    tool_names: &[String],
) -> Result<Vec<serde_json::Value>, String> {
    if tool_names.is_empty() {
        return Ok(vec![]);
    }

    #[derive(sqlx::FromRow)]
    struct FnRow {
        name:         String,
        description:  Option<String>,
        input_schema: Option<serde_json::Value>,
    }

    let rows = sqlx::query_as::<_, FnRow>(
        "SELECT name, description, input_schema
         FROM functions
         WHERE name = ANY($1)",
    )
    .bind(tool_names)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("db: {}", e))?;

    let schemas = rows
        .into_iter()
        .map(|r| {
            let parameters = r.input_schema.unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {}
            }));

            serde_json::json!({
                "type": "function",
                "function": {
                    "name":        r.name,
                    "description": r.description.unwrap_or_default(),
                    "parameters":  parameters,
                }
            })
        })
        .collect();

    Ok(schemas)
}
