//! Agent definition — parsed from YAML config files.
//!
//! Example YAML:
//! ```yaml
//! name: bug-report-agent
//! model: gpt-4o-mini
//! llm_url: https://api.openai.com/v1/chat/completions
//! llm_secret: OPENAI_KEY          # name of the project secret
//! system: |
//!   You are a bug triage agent. Analyse the report and create a GitHub issue.
//! tools:
//!   - create_github_issue
//!   - notify_slack
//! max_turns: 10
//! temperature: 0.3
//! rules:
//!   - { before: notify_slack, require: create_github_issue }
//!   - { tool: notify_slack, max_calls: 1 }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name:       String,
    pub model:      String,
    pub system:     String,
    pub tools:      Vec<String>,

    /// Chat completions endpoint (OpenAI-compatible).
    /// Defaults to OpenAI if not specified.
    #[serde(default = "default_llm_url")]
    pub llm_url:    String,

    /// Name of the project secret that holds the LLM API key.
    /// e.g. "OPENAI_KEY" → the agent reads secrets["OPENAI_KEY"] at runtime.
    #[serde(default = "default_llm_secret")]
    pub llm_secret: String,

    #[serde(default = "default_max_turns")]
    pub max_turns:  u32,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default)]
    pub config: Option<ModelConfig>,

    /// JSON Schema for the input payload (optional — not validated in v1).
    #[serde(default)]
    pub input_schema:  Option<JsonValue>,

    /// JSON Schema for the expected output (optional).
    #[serde(default)]
    pub output_schema: Option<JsonValue>,

    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfig {
    pub top_p:      Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Guard-rail rule.  Two variants:
///   `require`   — before calling tool X, require tool Y was already called.
///   `max_calls` — tool X may be called at most N times per agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rule {
    Require  { before: String, require: String },
    MaxCalls { tool: String, max_calls: u32 },
}

fn default_llm_url()    -> String { "https://api.openai.com/v1/chat/completions".into() }
fn default_llm_secret() -> String { "FLUXBASE_LLM_KEY".into() }
fn default_max_turns()  -> u32    { 25 }
fn default_temperature()-> f32    { 0.7 }

/// Parse an agent definition from a YAML string.
pub fn parse(yaml: &str) -> Result<AgentDefinition, serde_yaml::Error> {
    serde_yaml::from_str(yaml)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parse_applies_defaults() {
        let agent = parse(
            r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: []
"#,
        )
        .expect("yaml should parse");

        assert_eq!(agent.name, "smoke-agent");
        assert_eq!(agent.llm_secret, "FLUXBASE_LLM_KEY");
        assert_eq!(agent.llm_url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(agent.max_turns, 25);
        assert_eq!(agent.temperature, 0.7);
        assert!(agent.tools.is_empty());
    }
}
