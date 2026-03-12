/// Integrations + Tools API
///
/// Exposes the tool catalog and manages OAuth connections to external apps.
/// The runtime's ToolExecutor calls Composio directly — this module only
/// handles the management plane: connect, list, callback.
///
/// Composio stores OAuth tokens.  We store connection metadata in `integrations`.

use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use crate::types::response::{ApiResponse, ApiError};
use crate::types::context::RequestContext;
use crate::AppState;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use serde_json::Value;

const COMPOSIO_BASE_URL: &str = "https://backend.composio.dev/api/v2";

// ── Static tool catalog ────────────────────────────────────────────────────
//
// Mirrors runtime/src/tools/registry.rs.  The single source of truth for the
// developer-facing names ("slack.send_message") and which provider they belong to.

#[derive(Debug, Serialize, Clone)]
struct ToolCatalogItem {
    name:        &'static str,
    provider:    &'static str,
    label:       &'static str,
    description: &'static str,
}

static TOOL_CATALOG: &[ToolCatalogItem] = &[
    // Slack
    ToolCatalogItem { name: "slack.send_message",    provider: "slack",  label: "Send Message",        description: "Send a message to a Slack channel or DM" },
    ToolCatalogItem { name: "slack.create_channel",  provider: "slack",  label: "Create Channel",      description: "Create a new Slack channel" },
    ToolCatalogItem { name: "slack.get_messages",    provider: "slack",  label: "Get Messages",        description: "Fetch messages from a Slack channel" },
    // GitHub
    ToolCatalogItem { name: "github.create_issue",   provider: "github", label: "Create Issue",        description: "Create a new GitHub issue" },
    ToolCatalogItem { name: "github.close_issue",    provider: "github", label: "Close Issue",         description: "Close an existing GitHub issue" },
    ToolCatalogItem { name: "github.comment_issue",  provider: "github", label: "Comment on Issue",    description: "Add a comment to a GitHub issue" },
    ToolCatalogItem { name: "github.create_pr",      provider: "github", label: "Create Pull Request", description: "Open a new pull request" },
    ToolCatalogItem { name: "github.merge_pr",       provider: "github", label: "Merge Pull Request",  description: "Merge a pull request" },
    // Gmail
    ToolCatalogItem { name: "gmail.send_email",      provider: "gmail",  label: "Send Email",          description: "Send an email via Gmail" },
    ToolCatalogItem { name: "gmail.get_emails",      provider: "gmail",  label: "Get Emails",          description: "Fetch recent emails from Gmail" },
    // Linear
    ToolCatalogItem { name: "linear.create_issue",   provider: "linear", label: "Create Issue",        description: "Create a new Linear issue" },
    ToolCatalogItem { name: "linear.update_issue",   provider: "linear", label: "Update Issue",        description: "Update an existing Linear issue" },
    // Notion
    ToolCatalogItem { name: "notion.create_page",    provider: "notion", label: "Create Page",         description: "Create a new Notion page" },
    ToolCatalogItem { name: "notion.search",         provider: "notion", label: "Search",              description: "Search Notion workspace" },
    // Jira
    ToolCatalogItem { name: "jira.create_issue",     provider: "jira",   label: "Create Issue",        description: "Create a new Jira issue" },
    ToolCatalogItem { name: "jira.update_issue",     provider: "jira",   label: "Update Issue",        description: "Update an existing Jira issue" },
    ToolCatalogItem { name: "jira.comment_issue",    provider: "jira",   label: "Comment on Issue",    description: "Add a comment to a Jira issue" },
    // Airtable
    ToolCatalogItem { name: "airtable.create_record", provider: "airtable", label: "Create Record",   description: "Create a new Airtable record" },
    ToolCatalogItem { name: "airtable.list_records",  provider: "airtable", label: "List Records",     description: "List records from an Airtable base" },
    // Google Sheets
    ToolCatalogItem { name: "sheets.append_row",     provider: "googlesheets", label: "Append Row",   description: "Append a row to a Google Sheet" },
    // Stripe
    ToolCatalogItem { name: "stripe.create_customer", provider: "stripe", label: "Create Customer",   description: "Create a new Stripe customer" },
    ToolCatalogItem { name: "stripe.create_charge",   provider: "stripe", label: "Create Charge",     description: "Create a Stripe charge" },
];

// ── DB row ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Debug)]
struct IntegrationRow {
    id:                     Uuid,
    provider:               String,
    account_label:          Option<String>,
    composio_connection_id: Option<String>,
    status:                 String,
    metadata:               serde_json::Value,
    connected_at:           Option<chrono::DateTime<chrono::Utc>>,
    created_at:             chrono::DateTime<chrono::Utc>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn get_composio_key() -> Result<String, ApiError> {
    std::env::var("COMPOSIO_API_KEY")
        .or_else(|_| std::env::var("FLUXBASE_COMPOSIO_KEY"))
        .map_err(|_| ApiError::internal("composio_key_not_configured"))
}

// ── 1. GET /tools ──────────────────────────────────────────────────────────
//
// List all available tools, annotated with whether the provider is connected
// in this project.

pub async fn list_tools(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Value> {
    let project_id = context.project_id;

    // Fetch connected providers for this project
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT provider FROM integrations WHERE project_id = $1 AND status = 'active'"
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?;

    let connected: std::collections::HashSet<&str> = rows
        .iter()
        .map(|(p,)| p.as_str())
        .collect();

    let tools: Vec<Value> = TOOL_CATALOG.iter().map(|t| {
        serde_json::json!({
            "name":       t.name,
            "provider":   t.provider,
            "label":      t.label,
            "description": t.description,
            "connected":  connected.contains(t.provider),
        })
    }).collect();

    Ok(ApiResponse::new(serde_json::json!({ "tools": tools })))
}

// ── 2. GET /tools/connected ────────────────────────────────────────────────
//
// List only the integrations that are currently active for this project.

pub async fn list_connected(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Value> {
    let project_id = context.project_id;

    let rows = sqlx::query_as::<_, IntegrationRow>(
        "SELECT id, provider, account_label, composio_connection_id, status, metadata, connected_at, created_at \
         FROM integrations WHERE project_id = $1 ORDER BY created_at DESC"
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?;

    let connected: Vec<Value> = rows.into_iter().map(|r| {
        serde_json::json!({
            "id":            r.id,
            "provider":      r.provider,
            "account_label": r.account_label,
            "status":        r.status,
            "connected_at":  r.connected_at,
            "created_at":    r.created_at,
        })
    }).collect();

    Ok(ApiResponse::new(serde_json::json!({ "connected": connected })))
}

// ── 3. POST /tools/connect/:provider ──────────────────────────────────────
//
// Start an OAuth flow for a provider.  Creates a pending integration record,
// calls Composio to get the OAuth redirect URL, returns it to the caller.
// The dashboard redirects the user to this URL.

#[derive(Deserialize)]
pub struct ConnectPayload {
    /// Where to land the user after OAuth completes.
    /// Defaults to the dashboard integrations page.
    pub redirect_url: Option<String>,
}

pub async fn connect_provider(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(provider): Path<String>,
    Json(payload): Json<Option<ConnectPayload>>,
) -> ApiResult<Value> {
    let tenant_id  = context.tenant_id;
    let project_id = context.project_id;
    let api_key    = get_composio_key()?;

    // entity_id = tenant_id — Composio scopes credentials per entity
    let entity_id = tenant_id.to_string();

    let redirect_url = payload
        .as_ref()
        .and_then(|p| p.redirect_url.as_deref())
        .unwrap_or("https://app.fluxbase.co/dashboard/integrations");

    // Upsert pending integration (idempotent — let the user retry)
    sqlx::query(
        "INSERT INTO integrations (tenant_id, project_id, provider, status)
         VALUES ($1, $2, $3, 'pending')
         ON CONFLICT (project_id, provider) DO UPDATE SET status = 'pending', connected_at = NULL"
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(&provider)
    .execute(&state.pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?;

    // Ask Composio for the OAuth redirect URL
    let composio_url = format!("{}/connectedAccounts", COMPOSIO_BASE_URL);

    let body = serde_json::json!({
        "entityId":    entity_id,
        "appName":     provider.to_uppercase(),
        "redirectUri": redirect_url,
    });

    let response = state.http_client
        .post(&composio_url)
        .header("x-api-key", &api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(&format!("composio_connect: {}", e)))?;

    if !response.status().is_success() {
        let err = response.text().await.unwrap_or_default();
        return Err(ApiError::internal(&format!("composio_connect_error: {}", err)));
    }

    let result: Value = response.json().await
        .map_err(|_| ApiError::internal("composio_connect_parse"))?;

    let oauth_url = result
        .get("redirectUrl")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::internal("composio_connect: no redirectUrl"))?;

    Ok(ApiResponse::new(serde_json::json!({
        "provider":   provider,
        "oauth_url":  oauth_url,
        "status":     "pending",
    })))
}

// ── 4. GET /tools/oauth/callback ──────────────────────────────────────────
//
// Composio redirects here after OAuth completes.
// We update the integration record to "active" and record the connectionId.

#[derive(Deserialize)]
pub struct OAuthCallbackQuery {
    /// Composio-provided fields on successful OAuth completion
    #[serde(rename = "connectedAccountId")]
    pub connected_account_id: Option<String>,
    #[serde(rename = "connectionId")]
    pub connection_id: Option<String>,
    /// The entity_id we passed to Composio (= tenant_id)
    #[serde(rename = "entityId")]
    pub entity_id: Option<String>,
    /// App name Composio echoes back
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<OAuthCallbackQuery>,
) -> ApiResult<Value> {
    let connection_id = params.connected_account_id
        .as_deref()
        .or(params.connection_id.as_deref())
        .ok_or(ApiError::bad_request("missing_connection_id"))?;

    let entity_id = params.entity_id
        .ok_or(ApiError::bad_request("missing_entity_id"))?;

    let app_name = params.app_name
        .as_deref()
        .unwrap_or("unknown")
        .to_lowercase();

    // entity_id is the tenant_id string
    let tenant_id = Uuid::parse_str(&entity_id)
        .map_err(|_| ApiError::bad_request("invalid_entity_id"))?;

    // Fetch the connected account details from Composio to get the account label
    let api_key = get_composio_key()?;
    let label = fetch_account_label(&state.http_client, &api_key, connection_id).await;

    // Mark integration as active
    sqlx::query(
        "UPDATE integrations
         SET status = 'active',
             composio_connection_id = $1,
             account_label = $2,
             connected_at = NOW()
         WHERE tenant_id = $3 AND provider = $4 AND status = 'pending'"
    )
    .bind(connection_id)
    .bind(&label)
    .bind(tenant_id)
    .bind(&app_name)
    .execute(&state.pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?;

    Ok(ApiResponse::new(serde_json::json!({
        "provider":      app_name,
        "status":        "active",
        "connection_id": connection_id,
        "account_label": label,
    })))
}

// ── 5. DELETE /tools/connected/:provider ──────────────────────────────────

pub async fn disconnect_provider(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(provider): Path<String>,
) -> ApiResult<Value> {
    let project_id = context.project_id;

    let deleted = sqlx::query(
        "DELETE FROM integrations WHERE project_id = $1 AND provider = $2"
    )
    .bind(project_id)
    .bind(&provider)
    .execute(&state.pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?
    .rows_affected();

    if deleted == 0 {
        return Err(ApiError::not_found("integration_not_found"));
    }

    Ok(ApiResponse::new(serde_json::json!({ "provider": provider, "disconnected": true })))
}

// ── Composio helper ────────────────────────────────────────────────────────

async fn fetch_account_label(
    client:        &reqwest::Client,
    api_key:       &str,
    connection_id: &str,
) -> Option<String> {
    let url = format!("{}/connectedAccounts/{}", COMPOSIO_BASE_URL, connection_id);
    let Ok(response) = client
        .get(&url)
        .header("x-api-key", api_key)
        .send()
        .await
    else { return None; };

    if !response.status().is_success() { return None; }

    let Ok(body) = response.json::<Value>().await else { return None; };

    // Composio returns "displayName" or the entity label
    body.get("displayName")
        .or_else(|| body.get("accountLabel"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
