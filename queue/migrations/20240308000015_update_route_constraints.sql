-- Update routes table unique constraint to be project-scoped
-- This allows different projects to have the same paths (e.g. /auth)

ALTER TABLE routes DROP CONSTRAINT routes_path_method_key;
ALTER TABLE routes ADD CONSTRAINT routes_project_path_method_key UNIQUE (project_id, path, method);
