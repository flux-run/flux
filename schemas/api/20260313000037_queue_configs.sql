CREATE TABLE IF NOT EXISTS flux.queue_configs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id          UUID NOT NULL,
    name                TEXT NOT NULL,
    description         TEXT,
    max_attempts        INT NOT NULL DEFAULT 5,
    visibility_timeout_ms BIGINT NOT NULL DEFAULT 30000,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(project_id, name)
);
