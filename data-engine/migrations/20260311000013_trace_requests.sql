-- Request envelope store: makes replay fully self-contained.
--
-- Without this table, flux incident replay must fetch the original request
-- from the gateway or API log.  Gateway logs expire (typically 30–90 days)
-- and may be unavailable in isolated environments or incident post-mortems.
--
-- By writing the request envelope at the start of every POST /db/query,
-- the data engine owns the complete replay input:
--
--   trace_requests          ← original input (method, path, body, headers)
--         ↓
--   state_mutations         ← mutations ordered by mutation_seq
--         ↓
--   execution replay        ← re-apply mutations skipping side effects
--
-- Security note: raw Authorization headers and user credentials are NOT
-- stored.  Only the structural headers needed for replay:
--   x-user-id, x-user-role, x-tenant-id, x-project-id, x-flux-replay
--
-- response_body may be truncated for large result sets (first 64 KB).

CREATE TABLE IF NOT EXISTS fluxbase_internal.trace_requests (
    request_id      TEXT         PRIMARY KEY,     -- from x-request-id header; TEXT to accept any format
    tenant_id       UUID         NOT NULL,
    project_id      UUID         NOT NULL,

    method          TEXT         NOT NULL,         -- 'POST', 'GET', etc.
    path            TEXT         NOT NULL,         -- e.g. '/db/query'
    headers         JSONB,                         -- structural headers only (no auth tokens)
    body            JSONB,                         -- QueryRequest body; compressed via TOAST

    response_status INT,                           -- HTTP status returned to caller
    response_body   JSONB,                         -- response payload; may be truncated
    duration_ms     INT,                           -- end-to-end latency in ms

    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Tenant/project scoped time-range scan: list all requests in a window.
CREATE INDEX IF NOT EXISTS idx_trace_requests_tenant
    ON fluxbase_internal.trace_requests (tenant_id, project_id, created_at DESC);
