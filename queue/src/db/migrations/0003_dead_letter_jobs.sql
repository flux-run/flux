CREATE TABLE IF NOT EXISTS dead_letter_jobs (
    id UUID PRIMARY KEY,
    tenant_id UUID,
    project_id UUID,
    function_id UUID,
    payload JSONB,
    error TEXT,
    failed_at TIMESTAMP DEFAULT now()
);
