-- Demo infrastructure: tracks demo signups and enables public trace lookup.
-- Users submitted via the landing-page "Try Fluxbase" form.

CREATE TABLE IF NOT EXISTS demo_users (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL,
    email      TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Dedup: one demo slot per email address (soft, not a hard unique constraint).
-- We keep duplicates for analysis but reject >3 submissions from the same email.
CREATE INDEX IF NOT EXISTS idx_demo_users_email ON demo_users(email);

-- Each landing-page demo submission.
-- request_id matches the x-request-id that flows through the full trace.
CREATE TABLE IF NOT EXISTS demo_requests (
    request_id TEXT        PRIMARY KEY,
    ip         TEXT        NOT NULL,
    email      TEXT        NOT NULL,
    name       TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Rate-limit by IP (max 5 per 60 seconds).
CREATE INDEX IF NOT EXISTS idx_demo_requests_ip_time ON demo_requests(ip, created_at DESC);
-- Public trace lookup by request_id is the primary key — no extra index needed.
