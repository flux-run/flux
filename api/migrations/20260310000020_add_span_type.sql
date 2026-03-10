-- Add span_type to platform_logs for structured trace rendering.
-- Values: start | end | error | event (NULL treated as "event" by the read path)
ALTER TABLE platform_logs
    ADD COLUMN IF NOT EXISTS span_type TEXT;
