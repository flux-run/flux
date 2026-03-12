-- Trace Signatures table
--
-- Deterministic behavioral fingerprint of each request execution.
-- Enables automatic regression detection via binary commit search.
--
-- Columns
-- -------
--   id                 – unique ID (PK)
--   request_id         – x-request-id linking to the originating HTTP request
--   function_id        – the function that was invoked
--   code_sha           – exact git commit SHA of deployed code
--   signature_hash     – deterministic hash of execution behavior
--                        (latency quartiles, error patterns, branch coverage)
--   status_code        – final HTTP status from function execution
--   latency_ms         – end-to-end execution time
--   error_type         – if failed: timeout | invalid_payload | runtime_error | etc.
--   error_message      – error details if applicable
--   created_at         – timestamp
--
-- Indexes
-- -------
--   Primary query: (function_id, created_at DESC) for `flux trace list`
--   Regression detection: (code_sha, function_id) for `flux bug bisect`
--   Signature lookup: (signature_hash, function_id) for behavior comparison

CREATE TABLE IF NOT EXISTS trace_signatures (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id          TEXT        NOT NULL,
    function_id         UUID        NOT NULL,
    code_sha            TEXT        NOT NULL,
    signature_hash      TEXT        NOT NULL,
    status_code         INT,
    latency_ms          INT,
    error_type          TEXT,
    error_message       TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_function_ts
    ON trace_signatures (function_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_code_sha
    ON trace_signatures (code_sha, function_id);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_signature_hash
    ON trace_signatures (signature_hash, function_id);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_request_id
    ON trace_signatures (request_id);

