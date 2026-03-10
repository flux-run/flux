/// LLM Client — OpenAI-compatible chat completions.
///
/// Called by op_agent_llm_call to power the agent's reasoning loop.
/// Supports OpenAI and any compatible endpoint via FLUXBASE_LLM_URL.
///
/// Default endpoint : https://api.openai.com/v1/chat/completions
/// Default model    : gpt-4o-mini  (fast + cheap for tool-calling automation)
///
/// Response contract — the LLM must return JSON matching one of:
///   { "done": false, "tool": "slack.send_message", "input": { ... } }
///   { "done": true,  "answer": "summary of what was accomplished" }
///
/// If the LLM returns non-JSON text it is interpreted as done=true with
/// the text as the answer.

use serde_json::Value;

/// Call the LLM and return the agent's next action decision.
pub async fn call_llm(
    api_key:  &str,
    base_url: &str,
    model:    &str,
    messages: Value,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("llm_client_build: {}", e))?;

    let body = serde_json::json!({
        "model":           model,
        "messages":        messages,
        "response_format": { "type": "json_object" },
        "temperature":     0.1,
        "max_tokens":      512,
    });

    let response = client
        .post(base_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("llm_request: {}", e))?;

    let status = response.status();

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(
            "llm_auth: API key rejected. Set FLUXBASE_LLM_KEY in your Fluxbase secrets."
                .to_string(),
        );
    }

    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("llm_error({}): {}", status, body_text));
    }

    let resp: Value = response
        .json()
        .await
        .map_err(|e| format!("llm_parse: {}", e))?;

    // Extract content from choices[0].message.content
    let content = resp
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "llm_parse: no content in response".to_string())?;

    // Parse the JSON decision from the LLM.
    // If it can't be parsed as JSON, treat as a plain "done" text answer.
    let decision: Value = serde_json::from_str(content)
        .unwrap_or_else(|_| serde_json::json!({ "done": true, "answer": content }));

    Ok(decision)
}
