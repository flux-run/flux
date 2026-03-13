-- Replace function_id UUID FK with function_name text in gateway_trace_requests.
-- Routes now identify functions by name (not UUID), so the trace table must match.

ALTER TABLE flux.gateway_trace_requests
    DROP COLUMN IF EXISTS function_id;

ALTER TABLE flux.gateway_trace_requests
    ADD COLUMN IF NOT EXISTS function_name TEXT NOT NULL DEFAULT '';

DROP INDEX IF EXISTS flux.idx_gateway_trace_function;
CREATE INDEX idx_gateway_trace_function
    ON flux.gateway_trace_requests (function_name, created_at DESC);
