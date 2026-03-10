/// Tool Registry
///
/// Maps Fluxbase tool names (e.g. "slack.send_message") to the underlying
/// provider representation. The registry is the only place that knows about
/// Composio action IDs — everything else uses the Fluxbase name.
///
/// Format: "{app}.{action}"  →  COMPOSIO: "{APP}_{ACTION_SNAKE}"
///
/// The registry is intentionally decoupled so we can swap providers or add
/// native implementations without changing the ctx.tools.run() developer API.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// All metadata Fluxbase tracks about a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMeta {
    /// Canonical Fluxbase name shown to developers: "slack.send_message"
    pub name: String,
    /// Human-readable label: "Send a Slack message"
    pub label: String,
    /// App category: "slack", "github", "gmail", …
    pub app: String,
    /// Composio action ID: "SLACK_SEND_MESSAGE"
    pub composio_action: String,
    /// Brief description of what the tool does
    pub description: String,
}

/// The registry of all available tools.
///
/// In Phase 1 this is a static in-memory map loaded at startup.
/// In future phases this can be driven from the Fluxbase dashboard (dynamic
/// tool discovery via Composio's /actions API).
pub struct ToolRegistry {
    tools: HashMap<String, ToolMeta>,
}

impl ToolRegistry {
    /// Build the registry with all known tools.
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        let entries: &[(&str, &str, &str, &str, &str)] = &[
            // ── Slack ──────────────────────────────────────────────────────────
            ("slack.send_message",      "Send a Slack message",       "slack",   "SLACK_SEND_MESSAGE",         "Post a message to a Slack channel or user"),
            ("slack.create_channel",    "Create a Slack channel",     "slack",   "SLACK_CREATE_CHANNEL",       "Create a new Slack channel"),
            ("slack.invite_to_channel", "Invite to Slack channel",    "slack",   "SLACK_INVITE_TO_CHANNEL",    "Invite users to a Slack channel"),

            // ── GitHub ─────────────────────────────────────────────────────────
            ("github.create_issue",     "Create a GitHub issue",      "github",  "GITHUB_CREATE_AN_ISSUE",     "Open a new issue on a GitHub repository"),
            ("github.create_pr",        "Create a GitHub PR",         "github",  "GITHUB_CREATE_A_PULL_REQUEST", "Open a pull request"),
            ("github.add_comment",      "Comment on GitHub issue/PR", "github",  "GITHUB_CREATE_ISSUE_COMMENT","Add a comment to an issue or PR"),
            ("github.list_issues",      "List GitHub issues",         "github",  "GITHUB_LIST_ISSUES",         "List open issues on a repository"),
            ("github.star_repo",        "Star a GitHub repo",         "github",  "GITHUB_STAR_A_REPOSITORY_FOR_THE_AUTHENTICATED_USER", "Star a GitHub repository"),

            // ── Gmail ──────────────────────────────────────────────────────────
            ("gmail.send_email",        "Send an email",              "gmail",   "GMAIL_SEND_EMAIL",           "Send an email via Gmail"),
            ("gmail.create_draft",      "Create email draft",         "gmail",   "GMAIL_CREATE_EMAIL_DRAFT",   "Create a draft email in Gmail"),

            // ── Linear ─────────────────────────────────────────────────────────
            ("linear.create_issue",     "Create a Linear issue",      "linear",  "LINEAR_CREATE_ISSUE",        "Create a new issue in Linear"),
            ("linear.update_issue",     "Update a Linear issue",      "linear",  "LINEAR_UPDATE_ISSUE",        "Update an existing issue in Linear"),

            // ── Notion ─────────────────────────────────────────────────────────
            ("notion.create_page",      "Create a Notion page",       "notion",  "NOTION_CREATE_PAGE",         "Create a new page in a Notion database"),
            ("notion.update_page",      "Update a Notion page",       "notion",  "NOTION_UPDATE_PAGE_PROPERTIES", "Update properties on an existing Notion page"),

            // ── Jira ───────────────────────────────────────────────────────────
            ("jira.create_issue",       "Create a Jira issue",        "jira",    "JIRA_CREATE_ISSUE",          "Create a new Jira issue"),
            ("jira.update_issue",       "Update a Jira issue",        "jira",    "JIRA_UPDATE_ISSUE",          "Update an existing Jira issue"),
            ("jira.add_comment",        "Add a Jira comment",         "jira",    "JIRA_ADD_COMMENT",           "Add a comment to a Jira issue"),

            // ── Airtable ───────────────────────────────────────────────────────
            ("airtable.create_record",  "Create Airtable record",     "airtable","AIRTABLE_CREATE_RECORD",     "Create a record in an Airtable table"),
            ("airtable.list_records",   "List Airtable records",      "airtable","AIRTABLE_LIST_RECORDS",       "List records from an Airtable table"),

            // ── Google Sheets ──────────────────────────────────────────────────
            ("sheets.append_row",       "Append row to Sheet",        "sheets",  "GOOGLESHEETS_SHEET_FROM_SPREADSHEET", "Append a row to a Google Sheet"),

            // ── Stripe ─────────────────────────────────────────────────────────
            ("stripe.create_customer",  "Create Stripe customer",     "stripe",  "STRIPE_CREATE_CUSTOMER",     "Create a customer in Stripe"),
            ("stripe.create_invoice",   "Create Stripe invoice",      "stripe",  "STRIPE_CREATE_INVOICE",      "Create an invoice in Stripe"),
        ];

        for (name, label, app, composio_action, description) in entries {
            tools.insert(name.to_string(), ToolMeta {
                name:            name.to_string(),
                label:           label.to_string(),
                app:             app.to_string(),
                composio_action: composio_action.to_string(),
                description:     description.to_string(),
            });
        }

        Self { tools }
    }

    /// Look up a tool by its Fluxbase name ("slack.send_message").
    pub fn get(&self, name: &str) -> Option<&ToolMeta> {
        self.tools.get(name)
    }

    /// All registered tools (for dashboard listing, type generation, etc.).
    pub fn all(&self) -> Vec<&ToolMeta> {
        self.tools.values().collect()
    }

    /// Resolve a Fluxbase tool name to the Composio action ID.
    /// Falls back to auto-converting "app.action_name" → "APP_ACTION_NAME" if
    /// the tool is not in the static registry (forward-compat for new tools
    /// discovered dynamically from Composio's API).
    pub fn resolve_composio_action(&self, name: &str) -> String {
        if let Some(meta) = self.get(name) {
            return meta.composio_action.clone();
        }
        // Auto-convert: "slack.send_message" → "SLACK_SEND_MESSAGE"
        name.replace('.', "_").to_uppercase()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
