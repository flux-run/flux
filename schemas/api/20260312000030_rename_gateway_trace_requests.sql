-- Rename trace_requests → gateway_trace_requests and drop tenant columns.
--
-- Rationale:
--   1. Table naming convention: prefix by owning service (gateway_*)
--   2. No tenant_id — the gateway routes by (method, path) only, no tenant concept
--   3. function_version and artifact_uri are runtime concerns, not gateway concerns
--      The gateway only records the request envelope; the runtime records execution detail
--
-- The gateway INSERT (trace/mod.rs) is append-only and uses ON CONFLICT DO NOTHING.

-- Rename
ALTER TABLE IF EXISTS flux.trace_requests
    RENAME TO gateway_trace_requests;

-- Drop columns that don't belong at the gateway layer
ALTER TABLE flux.gateway_trace_requests
    DROP COLUMN IF EXISTS tenant_id,
    DROP COLUMN IF EXISTS function_version,
    DROP COLUMN IF EXISTS artifact_uri;

-- Recreate indexes under the new name
-- (Postgres keeps the old index names after rename — drop and recreate for clarity)
DROP INDEX IF EXISTS flux.idx_trace_requests_tenant_project_ts;
DROP INDEX IF EXISTS flux.idx_trace_requests_function_ts;
DROP INDEX IF EXISTS flux.idx_trace_requests_created_at;

CREATE INDEX IF NOT EXISTS idx_gateway_trace_project_ts
    ON flux.gateway_trace_requests (project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_gateway_trace_function_ts
    ON flux.gateway_trace_requests (function_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_gateway_trace_created_at
    ON flux.gateway_trace_requests (created_at ASC);
