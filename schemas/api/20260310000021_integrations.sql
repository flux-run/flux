-- Integrations: tracks which external apps a project has connected via OAuth.
-- Composio stores the actual OAuth tokens; we store the connection metadata here
-- so the dashboard can show what's connected and when.
--
-- Key design points:
--   • provider is the short app name: "slack", "github", "gmail", etc.
--   • composio_connection_id is the ID Composio returns after OAuth completes —
--     used to reference the connection if we ever need to refresh/revoke.
--   • entity_id mirrors Composio's entity_id (= tenant_id) for cross-referencing.
--   • status: pending → active (OAuth done) | error (something failed)
--   • ONE active connection per (project_id, provider).

CREATE TABLE integrations (
    id                      UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id               UUID         NOT NULL REFERENCES tenants(id)  ON DELETE CASCADE,
    project_id              UUID         NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider                VARCHAR(100) NOT NULL,
    account_label           VARCHAR(255),
    composio_connection_id  VARCHAR(255),
    status                  VARCHAR(50)  NOT NULL DEFAULT 'pending',
    metadata                JSONB        NOT NULL DEFAULT '{}',
    connected_at            TIMESTAMPTZ,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, provider)
);

CREATE INDEX idx_integrations_project ON integrations(project_id);
CREATE INDEX idx_integrations_tenant  ON integrations(tenant_id);
