-- 10. Platform Runtimes
CREATE TABLE IF NOT EXISTS platform_runtimes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    engine TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'disabled',
    version TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

-- Seed runtimes
INSERT INTO platform_runtimes (name, engine, status, version) VALUES
    ('deno', 'rust_deno_engine', 'active', '1.0.0'),
    ('nodejs', 'node_runtime', 'disabled', '20.x'),
    ('python', 'python_runtime', 'disabled', '3.11')
ON CONFLICT (name) DO NOTHING;

-- 11. Platform Services
CREATE TABLE IF NOT EXISTS platform_services (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'disabled',
    created_at TIMESTAMP DEFAULT NOW()
);

-- Seed services
INSERT INTO platform_services (name, status) VALUES
    ('serverless', 'active'),
    ('events', 'active'),
    ('database', 'disabled'),
    ('workflow', 'disabled'),
    ('storage', 'disabled')
ON CONFLICT (name) DO NOTHING;

-- 12. Platform Limits (Quotas per tenant)
CREATE TABLE IF NOT EXISTS platform_limits (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    max_functions INT NOT NULL DEFAULT 10,
    max_deployments INT NOT NULL DEFAULT 50,
    max_storage_mb INT NOT NULL DEFAULT 1000,
    max_requests_per_month BIGINT NOT NULL DEFAULT 100000,
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Update deployments table with a status column
ALTER TABLE deployments ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'ready';
