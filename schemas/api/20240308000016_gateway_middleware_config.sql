-- JWT Middleware Config
ALTER TABLE routes ADD COLUMN IF NOT EXISTS jwks_url TEXT;
ALTER TABLE routes ADD COLUMN IF NOT EXISTS jwt_audience TEXT;
ALTER TABLE routes ADD COLUMN IF NOT EXISTS jwt_issuer TEXT;

-- Validation Middleware Config
ALTER TABLE routes ADD COLUMN IF NOT EXISTS json_schema JSONB;

-- Advanced CORS Middleware Config
ALTER TABLE routes ADD COLUMN IF NOT EXISTS cors_origins TEXT[];
ALTER TABLE routes ADD COLUMN IF NOT EXISTS cors_headers TEXT[];

-- Analytics Table
CREATE TABLE IF NOT EXISTS gateway_metrics (
  id UUID PRIMARY KEY,
  route_id UUID,
  tenant_id UUID,
  status INT,
  latency_ms INT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
