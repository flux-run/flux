-- Trace context propagation: store the originating request_id on every event.
--
-- Without this column the event worker cannot forward x-request-id when it
-- dispatches to webhooks or runtime functions, breaking the trace chain:
--
--   Gateway → Runtime → Data Engine → Hooks/Events ← (broken before this)
--
-- After this migration the worker reads request_id from the events row and
-- forwards it as the x-request-id header on every outbound call, making the
-- full trace chain continuous.

ALTER TABLE fluxbase_internal.events
    ADD COLUMN IF NOT EXISTS request_id TEXT;
