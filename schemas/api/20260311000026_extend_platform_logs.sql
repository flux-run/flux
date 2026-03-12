-- Extend platform_logs with tracing and code provenance columns
--
-- Adds critical fields for:
--   - Structured span hierarchy (parent_span_id)
--   - Code provenance (code_sha, code_location)
--   - Local variable inspection (execution_state)
--   - Step-through debugging (checkpoint_type)

ALTER TABLE platform_logs
    ADD COLUMN IF NOT EXISTS parent_span_id UUID,
    ADD COLUMN IF NOT EXISTS span_id UUID,
    ADD COLUMN IF NOT EXISTS code_sha TEXT,
    ADD COLUMN IF NOT EXISTS code_location TEXT,
    ADD COLUMN IF NOT EXISTS checkpoint_type TEXT,
    ADD COLUMN IF NOT EXISTS execution_state JSONB;

-- Span hierarchy index for trace reconstruction
CREATE INDEX IF NOT EXISTS idx_platform_logs_parent_span_id
    ON platform_logs (parent_span_id) WHERE parent_span_id IS NOT NULL;

-- Code provenance index for blame lookups
CREATE INDEX IF NOT EXISTS idx_platform_logs_code_sha
    ON platform_logs (code_sha) WHERE code_sha IS NOT NULL;

-- Checkpoint index for step-through debugger
CREATE INDEX IF NOT EXISTS idx_platform_logs_checkpoint_type
    ON platform_logs (checkpoint_type) WHERE checkpoint_type IS NOT NULL;

