-- Create routes table for gateway routing
CREATE TABLE IF NOT EXISTS routes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    method TEXT NOT NULL,
    function_id UUID NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    auth_type TEXT NOT NULL DEFAULT 'none', -- 'none', 'api_key', 'jwt'
    cors_enabled BOOLEAN NOT NULL DEFAULT false,
    rate_limit INTEGER, -- Requests per minute, NULL for unlimited
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(path, method) -- Ensure unique routing per (path, method) pair
);

-- Index for fast lookup by path and method
CREATE INDEX IF NOT EXISTS idx_routes_path_method ON routes(path, method);
