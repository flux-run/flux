-- Function execution logs (populated by runtime after each invocation)
CREATE TABLE IF NOT EXISTS function_logs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    function_id UUID NOT NULL,
    level       TEXT NOT NULL DEFAULT 'info',
    message     TEXT NOT NULL,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for fast log retrieval by function
CREATE INDEX IF NOT EXISTS idx_function_logs_function_id ON function_logs(function_id, timestamp DESC);
