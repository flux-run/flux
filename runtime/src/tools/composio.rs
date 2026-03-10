/// Composio Adapter
///
/// This is the ONLY file in Fluxbase that knows about Composio.
/// All other code calls tool_executor.run() — this file is the seam.
/// Replace this file to swap Composio for any other provider.
///
/// Composio REST API reference:
///   POST /api/v2/actions/{actionName}/execute
///   Headers: x-api-key: {COMPOSIO_API_KEY}
///   Body:    { "entityId": "...", "input": { ...action_params... } }

use serde::{Deserialize, Serialize};
use serde_json::Value;

const COMPOSIO_BASE_URL: &str = "https://backend.composio.dev/api/v2";

/// Result from calling a Composio action.
#[derive(Debug, Serialize, Deserialize)]
pub struct ComposioResult {
    /// Whether the action succeeded (Composio's own success flag)
    pub successful: bool,
    /// Composio's error message if successful=false
    pub error: Option<String>,
    /// The action's output data
    pub data: Option<Value>,
}

/// Call a Composio action.
///
/// # Arguments
/// - `api_key`      Fluxbase platform Composio key (stored as platform secret)
/// - `entity_id`    Per-tenant identifier — maps to a Composio "entity" which
///                  holds all connected accounts for that tenant  
/// - `action_name`  Composio action ID, e.g. "SLACK_SEND_MESSAGE"
/// - `app_name`     App slug e.g. "gmail", "slack" — required by Composio to
///                  resolve the correct connected account for the entity
/// - `input`        Action input parameters (free-form JSON)
pub async fn execute_action(
    api_key:     &str,
    entity_id:   &str,
    action_name: &str,
    app_name:    Option<&str>,
    input:       Value,
) -> Result<ComposioResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("composio_client_build: {}", e))?;

    let url = format!("{}/actions/{}/execute", COMPOSIO_BASE_URL, action_name);

    let mut body = serde_json::json!({
        "entityId": entity_id,
        "input":    input,
    });

    // Include appName so Composio can resolve the connected account for the entity
    if let Some(app) = app_name {
        if let serde_json::Value::Object(ref mut map) = body {
            map.insert("appName".to_string(), serde_json::json!(app));
        }
    }

    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("composio_request: {}", e))?;

    let status = response.status();

    if status == 401 || status == 403 {
        return Err(format!(
            "composio_auth: API key rejected or app not connected (HTTP {}). \
             Connect the app in your Fluxbase dashboard → Integrations.",
            status
        ));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("composio_error({}): {}", status, body));
    }

    let result: ComposioResult = response
        .json()
        .await
        .map_err(|e| format!("composio_parse: {}", e))?;

    if !result.successful {
        return Err(format!(
            "composio_action_failed({}): {}",
            action_name,
            result.error.as_deref().unwrap_or("unknown error")
        ));
    }

    Ok(result)
}

/// Fetch all connected accounts for an entity (used for dashboard listings).
pub async fn list_connected_accounts(
    api_key:   &str,
    entity_id: &str,
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/connectedAccounts?entityId={}", COMPOSIO_BASE_URL, entity_id);

    let response = client
        .get(&url)
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| format!("composio_request: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("composio_error({})", response.status()));
    }

    response.json::<Value>()
        .await
        .map_err(|e| format!("composio_parse: {}", e))
}

/// Initiate an OAuth connection flow for an entity + app.
/// Returns the redirect URL the user must visit to grant access.
pub async fn connect_app(
    api_key:          &str,
    entity_id:        &str,
    app_name:         &str,
    redirect_url:     Option<&str>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/connectedAccounts", COMPOSIO_BASE_URL);

    let mut body = serde_json::json!({
        "entityId":        entity_id,
        "appName":         app_name,
    });

    if let Some(redirect) = redirect_url {
        body["redirectUri"] = serde_json::Value::String(redirect.to_string());
    }

    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("composio_connect_request: {}", e))?;

    if !response.status().is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(format!("composio_connect_error: {}", err));
    }

    let result: Value = response.json().await
        .map_err(|e| format!("composio_connect_parse: {}", e))?;

    result
        .get("redirectUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "composio_connect: no redirectUrl in response".to_string())
}
