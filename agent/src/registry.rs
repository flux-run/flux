//! Agent registry — deploy, fetch, list, delete agents in the DB.

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::schema::AgentDefinition;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct AgentSummary {
    pub id:          Uuid,
    pub name:        String,
    pub model:       String,
    pub llm_url:     String,
    pub content_sha: String,
    pub deployed_at: chrono::DateTime<chrono::Utc>,
    pub updated_at:  chrono::DateTime<chrono::Utc>,
}

// ── Operations ────────────────────────────────────────────────────────────────

/// Parse YAML, validate tool names exist in DB, then upsert the agent record.
///
/// Returns the parsed definition so the caller can show a summary.
pub async fn deploy_from_yaml(pool: &PgPool, raw_yaml: &str) -> Result<AgentDefinition, String> {
    let agent = crate::schema::parse(raw_yaml)
        .map_err(|e| format!("yaml_parse: {}", e))?;

    // Validate: all listed tools must exist as functions
    if !agent.tools.is_empty() {
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM functions WHERE name = ANY($1)",
        )
        .bind(&agent.tools)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("db: {}", e))?;

        let missing: Vec<&String> = agent.tools
            .iter()
            .filter(|t| !existing.contains(t))
            .collect();

        if !missing.is_empty() {
            return Err(format!(
                "unknown_tools: {} — deploy the functions first",
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
    }

    let content_sha = {
        let mut h = Sha256::new();
        h.update(raw_yaml.as_bytes());
        hex::encode(h.finalize())
    };

    // Serialise optional JSONB fields
    let config_json = serde_json::to_value(
        agent.config.as_ref().unwrap_or(&crate::schema::ModelConfig::default())
    ).unwrap_or(serde_json::json!({}));

    let rules_json = serde_json::to_value(&agent.rules)
        .unwrap_or(serde_json::json!([]));

    sqlx::query(
        "INSERT INTO flux.agents
             (name, model, system, tools, llm_url, llm_secret,
              max_turns, temperature, config, input_schema, output_schema,
              rules, content_sha)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
         ON CONFLICT (name) DO UPDATE SET
             model        = EXCLUDED.model,
             system       = EXCLUDED.system,
             tools        = EXCLUDED.tools,
             llm_url      = EXCLUDED.llm_url,
             llm_secret   = EXCLUDED.llm_secret,
             max_turns    = EXCLUDED.max_turns,
             temperature  = EXCLUDED.temperature,
             config       = EXCLUDED.config,
             input_schema = EXCLUDED.input_schema,
             output_schema= EXCLUDED.output_schema,
             rules        = EXCLUDED.rules,
             content_sha  = EXCLUDED.content_sha,
             updated_at   = NOW()",
    )
    .bind(&agent.name)
    .bind(&agent.model)
    .bind(&agent.system)
    .bind(&agent.tools)
    .bind(&agent.llm_url)
    .bind(&agent.llm_secret)
    .bind(agent.max_turns as i32)
    .bind(agent.temperature)
    .bind(&config_json)
    .bind(&agent.input_schema)
    .bind(&agent.output_schema)
    .bind(&rules_json)
    .bind(&content_sha)
    .execute(pool)
    .await
    .map_err(|e| format!("db_upsert: {}", e))?;

    Ok(agent)
}

/// Load a single agent by name.  Returns `None` if not found.
pub async fn get_agent(pool: &PgPool, name: &str) -> Result<Option<AgentDefinition>, String> {
    #[derive(sqlx::FromRow)]
    struct Row {
        model:        String,
        system:       String,
        tools:        Vec<String>,
        llm_url:      String,
        llm_secret:   String,
        max_turns:    i32,
        temperature:  f32,
        config:       serde_json::Value,
        input_schema: Option<serde_json::Value>,
        output_schema:Option<serde_json::Value>,
        rules:        serde_json::Value,
    }

    let row = sqlx::query_as::<_, Row>(
        "SELECT model, system, tools, llm_url, llm_secret, max_turns, temperature,
                config, input_schema, output_schema, rules
         FROM flux.agents WHERE name = $1 LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("db: {}", e))?;

    let Some(r) = row else { return Ok(None) };

    let agent = AgentDefinition {
        name:         name.to_string(),
        model:        r.model,
        system:       r.system,
        tools:        r.tools,
        llm_url:      r.llm_url,
        llm_secret:   r.llm_secret,
        max_turns:    r.max_turns as u32,
        temperature:  r.temperature,
        config:       serde_json::from_value(r.config).ok(),
        input_schema: r.input_schema,
        output_schema:r.output_schema,
        rules:        serde_json::from_value(r.rules).unwrap_or_default(),
    };

    Ok(Some(agent))
}

/// List all deployed agents (summary only, no system prompt).
pub async fn list_agents(pool: &PgPool) -> Result<Vec<AgentSummary>, String> {
    sqlx::query_as::<_, AgentSummary>(
        "SELECT id, name, model, llm_url, content_sha, deployed_at, updated_at
         FROM flux.agents ORDER BY name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("db: {}", e))
}

/// Delete an agent by name.  Returns `true` if a row was deleted.
pub async fn delete_agent(pool: &PgPool, name: &str) -> Result<bool, String> {
    let result = sqlx::query("DELETE FROM flux.agents WHERE name = $1")
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| format!("db: {}", e))?;

    Ok(result.rows_affected() > 0)
}
