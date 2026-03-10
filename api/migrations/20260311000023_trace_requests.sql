-- Trace Requests table
--
-- Captures the complete request envelope for each user-initiated request.
-- This is the golden source for deterministic replay.
--
-- Columns
-- -------
--   request_id         – x-request-id from the HTTP request (PK)
--   tenant_id          – owning tenant
--   project_id         – owning project
--   function_id        – target function
--   function_version   – version of the function at execution time
--   method             – HTTP method (GET, POST, etc.)
--   path               – request path
--   headers            – all HTTP headers (JSONB, excludes auth tokens)
--   query_params       – URL query parameters (JSONB)
--   body               – request body (JSONB or NULL)
--   artifact_uri       – optional URI if body stored externally (>1MB payloads)
--   created_at         – timestamp of the request
--
-- Indexes
-- -------
--   Primary query: (tenant_id, project_id, created_at DESC) for list view
--   Replay lookup: (request_id) for `flux replay <id>`
--   Analytics: (function_id, created_at DESC) for function-level stats

CREATE TABLE IF NOT EXISTS trace_requests (
    request_id          TEXT        PRIMARY KEY,
    tenant_id           UUID        NOT NULL,
    project_id          UUID        NOT NULL,
    function_id         UUID        NOT NULL,
    function_version    TEXT,
    method              TEXT        NOT NULL,
    path                TEXT        NOT NULL,
    headers             JSONB,
    query_params        JSONB,
    body                JSONB,
    artifact_uri        TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_trace_requests_tenant_project_ts
    ON trace_requests (tenant_id, project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_trace_requests_function_ts
    ON trace_requests (function_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_trace_requests_created_at
    ON trace_requests (created_at ASC);

