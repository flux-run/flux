# Example: Webhook Worker

A compact async example that crosses an intake/replay boundary.

## Goal

- accept a webhook-style payload
- inspect the failed execution
- replay with field-level diff

## Steps

```bash
flux server start --database-url postgres://localhost:5432/postgres
flux init
flux serve webhook.ts
curl -sS -X POST http://127.0.0.1:3000/webhook \
  -H 'content-type: application/json' \
  -d '{"provider":"stripe","event":"invoice.paid"}'
flux logs --path /webhook --limit 20
flux trace <execution_id> --verbose
flux replay <execution_id> --diff
flux resume <execution_id>
```

## What to Look For

- trace shows full request payload and response/error
- replay output highlights changed JSON fields
- resume continues from checkpointed call boundaries when supported
