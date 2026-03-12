ALTER TABLE routes
ADD COLUMN IF NOT EXISTS is_async BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_routes_project_async
ON routes(project_id, is_async);
