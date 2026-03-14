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
        assert!(agent.rules.is_empty());
    }

    #[test]
    fn parse_fails_on_missing_required_fields() {
        // Missing "name"
        let yaml = r#"
model: gpt-4o-mini
system: You are a smoke test.
tools: []
"#;
        assert!(parse(yaml).is_err());

        // Missing "model"
        let yaml = r#"
name: smoke-agent
system: You are a smoke test.
tools: []
"#;
        assert!(parse(yaml).is_err());

        // Missing "system"
        let yaml = r#"
name: smoke-agent
model: gpt-4o-mini
tools: []
"#;
        assert!(parse(yaml).is_err());
    }

    #[test]
    fn parse_fails_on_invalid_types() {
        let yaml = r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: []
max_turns: "this should be a number, not a string"
"#;
        assert!(parse(yaml).is_err());

        let yaml = r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: "this should be an array"
"#;
        assert!(parse(yaml).is_err());
    }

    #[test]
    fn parse_fails_on_malformed_yaml() {
        let yaml = r#"
name: smoke-agent
model: gpt-4o-mini
system: [unclosed array
"#;
        assert!(parse(yaml).is_err());
    }

    #[test]
    fn parse_handles_unknown_fields_gracefully() {
        let agent = parse(
            r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: []
some_unknown_field: true
nested_unknown_field:
  - 1
  - 2
"#,
        )
        .expect("yaml should parse ignoring unknown fields");

        assert_eq!(agent.name, "smoke-agent");
    }

    #[test]
    fn parse_rules_variants() {
        let agent = parse(
            r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: []
rules:
  - { before: notify_slack, require: create_github_issue }
  - { tool: notify_slack, max_calls: 1 }
"#,
        )
        .expect("yaml with rules should parse");

        assert_eq!(agent.rules.len(), 2);
        
        match &agent.rules[0] {
            super::Rule::Require { before, require } => {
                assert_eq!(before, "notify_slack");
                assert_eq!(require, "create_github_issue");
            }
            _ => panic!("Expected Require rule"),
        }

        match &agent.rules[1] {
            super::Rule::MaxCalls { tool, max_calls } => {
                assert_eq!(tool, "notify_slack");
                assert_eq!(*max_calls, 1);
            }
            _ => panic!("Expected MaxCalls rule"),
        }
    }

    #[test]
    fn parse_invalid_rules() {
        let yaml = r#"
name: smoke-agent
model: gpt-4o-mini
system: You are a smoke test.
tools: []
rules:
  - { invalid_rule_key: true }
"#;
        // The enum is untagged, and both variants have required fields.
        // It should fail to parse an unrecognized format.
        assert!(parse(yaml).is_err());
    }

    #[test]
    fn parse_full_specification() {
        let yaml = r#"
name: full-agent
model: custom-model
system: You are a full agent.
tools: ["tool1", "tool2"]
llm_url: https://api.custom.com/v1/chat/completions
llm_secret: CUSTOM_KEY
max_turns: 50
temperature: 0.1
config:
  top_p: 0.9
  max_tokens: 1000
input_schema:
  type: object
  properties:
    text: { type: string }
output_schema:
  type: array
  items: { type: string }
"#;
        let agent = parse(yaml).expect("full spec should parse");

        assert_eq!(agent.name, "full-agent");
        assert_eq!(agent.model, "custom-model");
        assert_eq!(agent.system, "You are a full agent.");
        assert_eq!(agent.tools, vec!["tool1", "tool2"]);
        assert_eq!(agent.llm_url, "https://api.custom.com/v1/chat/completions");
        assert_eq!(agent.llm_secret, "CUSTOM_KEY");
        assert_eq!(agent.max_turns, 50);
        assert_eq!(agent.temperature, 0.1);
        
        let config = agent.config.unwrap();
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.max_tokens, Some(1000));
        
        assert!(agent.input_schema.is_some());
        assert!(agent.output_schema.is_some());
    }
}
