# Example: Webhook Worker

This example shows Flux as an event-ingestion and background-processing system.

It is a good demonstration of why queues, retries, and mutation history belong in the same product as request tracing.

## What The Example Covers

- a gateway endpoint for incoming webhooks
- signature verification or auth checks
- storing raw events for audit
- enqueueing follow-up work
- worker execution with retries
- linked traces between intake and background jobs

## Why This Example Matters

Many real backend incidents cross an async boundary.

The webhook example shows that Flux answers:

- which webhook created this job?
- which retry failed?
- what state changed?
- what did the follow-up worker do?

This is a strong proof of the complete-system story.

## Good Demo Flow

```bash
flux init webhook-worker
flux dev
flux invoke receive_webhook --gateway --payload '{"provider":"stripe","event":"invoice.paid"}'
flux trace
flux why <request_id>
flux queue
```

## What A Reader Should Learn

This example shows:

- why async work is part of Flux rather than an add-on
- how parent and child executions stay linked
- how retries and queue state fit into the debugging model
