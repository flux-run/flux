-- State Mutations table
--
-- Append-only event log of every backend state change linked to a request.
-- Enables time-travel debugging: reconstruct backend state at any moment.
--
-- Columns
-- -------
--   id                 – unique ID (PK)
--   request_id         – x-request-id linking to the originating HTTP request
--   span_id            – optional span_id if mutation originated from a span
--   tenant_id          – owning tenant
--   entity_type        – type of entity changed (user, order, workspace, etc.)
--   entity_id          – specific entity instance (user_123, order_456, etc.)
--   operation          – create | update | delete | patch
--   before             – state before mutation (JSONB or NULL for create)
--   after              – state after mutation (JSONB or NULL for delete)
--   source             – origin system (function, workflow, event, api, etc.)
--   created_at         – timestamp of mutation
--
-- Indexes
-- -------
--   Primary query: (request_id) for `flux state history <request-id>`
--   Time-travel: (entity_type, entity_id, created_at DESC) for `flux state blame`
--   Archival: (created_at ASC) for pruning old records

CREATE TABLE IF NOT EXISTS state_mutations (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id          TEXT        NOT NULL,
    span_id             UUID,
    tenant_id           UUID        NOT NULL,
    entity_type         TEXT        NOT NULL,
    entity_id           TEXT        NOT NULL,
    operation           TEXT        NOT NULL,
    before              JSONB,
    after               JSONB,
    source              TEXT        DEFAULT 'function',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_state_mutations_request_id
    ON state_mutations (request_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_state_mutations_entity
    ON state_mutations (entity_type, entity_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_state_mutations_created_at
    ON state_mutations (created_at ASC);

CREATE INDEX IF NOT EXISTS idx_state_mutations_tenant_ts
    ON state_mutations (tenant_id, created_at DESC);

