CREATE TABLE IF NOT EXISTS flux.environments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL,
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    config      JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(project_id, slug)
);
