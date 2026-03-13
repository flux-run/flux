//! OpenAI-compatible LLM client.
//!
//! Calls any chat completions endpoint that follows the OpenAI contract.
//! The URL and API key are passed per-call — no global state.
//!
//! Supports:
//!   - `tools` parameter (proper function-calling, not json_object mode)
//!   - `tool_calls` response parsing
//!   - `temperature`, `top_p`, `max_tokens` from agent config
//!   - Token usage tracking

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role:        String,
    pub content:     Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls:  Option<Vec<ToolCallReq>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id:Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name:        Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallReq {
    pub id:       String,
    #[serde(rename = "type")]
    pub kind:     String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name:      String,
    pub arguments: String,  // JSON string
}

impl ChatMessage {
    pub fn system(content: &str) -> Self {
        Self { role: "system".into(), content: Some(content.into()),
               tool_calls: None, tool_call_id: None, name: None }
    }
    pub fn user(content: &str) -> Self {
        Self { role: "user".into(), content: Some(content.into()),
               tool_calls: None, tool_call_id: None, name: None }
    }
    pub fn assistant_tool_calls(calls: Vec<ToolCallReq>) -> Self {
        Self { role: "assistant".into(), content: None,
               tool_calls: Some(calls), tool_call_id: None, name: None }
    }
    pub fn tool_result(call_id: &str, tool_name: &str, output: &str) -> Self {
        Self { role: "tool".into(), content: Some(output.into()),
               tool_calls: None, tool_call_id: Some(call_id.into()),
               name: Some(tool_name.into()) }
    }
    pub fn assistant_text(content: &str) -> Self {
        Self { role: "assistant".into(), content: Some(content.into()),
               tool_calls: None, tool_call_id: None, name: None }
    }
}

/// What the LLM decided to do.
#[derive(Debug)]
pub enum LlmResponse {
    /// LLM wants to call one or more tools.
    ToolUse {
        calls:             Vec<ToolCallReq>,
        prompt_tokens:     u32,
        completion_tokens: u32,
    },
    /// LLM produced a final text answer.
    FinalAnswer {
        content:           String,
        prompt_tokens:     u32,
        completion_tokens: u32,
    },
}

// ── LLM call ──────────────────────────────────────────────────────────────────

pub struct ChatRequest<'a> {
    pub model:        &'a str,
    pub messages:     &'a [ChatMessage],
    pub tools:        &'a [Value],      // tool schema objects
    pub temperature:  f32,
    pub top_p:        Option<f32>,
    pub max_tokens:   Option<u32>,
}

pub async fn chat(
    client:  &reqwest::Client,
    url:     &str,
    api_key: &str,
    req:     ChatRequest<'_>,
) -> Result<LlmResponse, String> {
    let mut body = serde_json::json!({
        "model":       req.model,
        "messages":    req.messages,
        "temperature": req.temperature,
    });

    if !req.tools.is_empty() {
        body["tools"] = serde_json::json!(req.tools);
        body["tool_choice"] = serde_json::json!("auto");
    }
    if let Some(p) = req.top_p      { body["top_p"] = p.into(); }
    if let Some(n) = req.max_tokens { body["max_tokens"] = n.into(); }

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("llm_request: {}", e))?;

    let status = response.status();

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(format!(
            "llm_auth: API key rejected ({}). Check the secret value.",
            status.as_u16()
        ));
    }

    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("llm_error({}): {}", status, text));
    }

    let resp: Value = response.json()
        .await
        .map_err(|e| format!("llm_parse: {}", e))?;

    // ── Extract token usage ───────────────────────────────────────────────
    let prompt_tokens     = resp["usage"]["prompt_tokens"]    .as_u64().unwrap_or(0) as u32;
    let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

    // ── Extract choice ────────────────────────────────────────────────────
    let choice  = &resp["choices"][0];
    let message = &choice["message"];
    let finish  = choice["finish_reason"].as_str().unwrap_or("");

    // Tool calls take priority
    if let Some(calls_raw) = message["tool_calls"].as_array() {
        if !calls_raw.is_empty() {
            let calls: Vec<ToolCallReq> = calls_raw
                .iter()
                .filter_map(|c| serde_json::from_value(c.clone()).ok())
                .collect();

            return Ok(LlmResponse::ToolUse { calls, prompt_tokens, completion_tokens });
        }
    }

    // Final text answer
    let content = message["content"]
        .as_str()
        .unwrap_or_else(|| {
            if finish == "stop" { "" } else { "no_content" }
        })
        .to_string();

    Ok(LlmResponse::FinalAnswer { content, prompt_tokens, completion_tokens })
}
