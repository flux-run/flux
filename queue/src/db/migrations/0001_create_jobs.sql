CREATE TABLE jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    project_id UUID NOT NULL,
    type TEXT NOT NULL,
    function_id UUID,
    payload JSONB,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INT DEFAULT 0,
    max_attempts INT DEFAULT 5,
    run_at TIMESTAMP NOT NULL DEFAULT now(),
    locked_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT now(),
    updated_at TIMESTAMP DEFAULT now()
);

CREATE INDEX idx_jobs_pending ON jobs(status, run_at);

CREATE TABLE job_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id UUID REFERENCES jobs(id),
    message TEXT,
    created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE dead_letter_jobs (
    id UUID PRIMARY KEY,
    tenant_id UUID,
    payload JSONB,
    error TEXT,
    failed_at TIMESTAMP DEFAULT now()
);