-- Unified platform-wide log table.
--
-- Replaces the function-only `function_logs` table with a single log primitive
-- that covers every system that can emit log lines: functions, databases,
-- workflows, events, queues, and internal platform services.
--
-- Columns
-- -------
--   tenant_id   – owning tenant (required for archival path partitioning)
--   project_id  – optional; NULL for platform-level (system) logs
--   source      – emitting subsystem:
--                   function | db | workflow | event | queue | system
--   resource_id – identifies the specific resource within the source:
--                   function name, table name, workflow id, queue name, …
--                   Empty string for source=system.
--   level       – log level: debug | info | warn | error
--   message     – log line text
--   request_id  – optional x-request-id from the originating HTTP call
--   metadata    – arbitrary structured data (latency_ms, status_code, etc.)
--   timestamp   – time of emission
--
-- Indexes
-- -------
--   Primary query: project+source+resource, newest-first
--   Follow mode  : project, newest-first (all sources)
--   Archival scan: timestamp only (for finding expired rows)

CREATE TABLE IF NOT EXISTS platform_logs (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID        NOT NULL,
    project_id  UUID,
    source      TEXT        NOT NULL DEFAULT 'function',
    resource_id TEXT        NOT NULL DEFAULT '',
    level       TEXT        NOT NULL DEFAULT 'info',
    message     TEXT        NOT NULL,
    request_id  TEXT,
    metadata    JSONB,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast lookup for "show me the last N logs for this function/db/etc."
CREATE INDEX IF NOT EXISTS idx_platform_logs_project_source_resource
    ON platform_logs (project_id, source, resource_id, timestamp DESC)
    WHERE project_id IS NOT NULL;

-- Fast lookup for "show me all recent logs for this project" (no source filter)
CREATE INDEX IF NOT EXISTS idx_platform_logs_project_ts
    ON platform_logs (project_id, timestamp DESC)
    WHERE project_id IS NOT NULL;

-- Archival scan: cheaply find all rows older than a cutoff
CREATE INDEX IF NOT EXISTS idx_platform_logs_timestamp
    ON platform_logs (timestamp ASC);

-- Tenant-scoped archival (used when hot_days window differs per-tenant in future)
CREATE INDEX IF NOT EXISTS idx_platform_logs_tenant_ts
    ON platform_logs (tenant_id, timestamp ASC);
