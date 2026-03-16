# Example: Webhook Replay

An example that walks through receiving a request, finding a failing execution, and using replay to verify a fix.

## Goal

- serve a webhook-style handler
- find a failing execution
- replay with field-level diff to confirm the fix

## Steps

```bash
# 1. Start the server
flux server start --database-url postgres://postgres:postgres@localhost:5432/flux

# 2. One-time auth setup
flux init

# 3. Serve the handler
flux serve webhook.js

# 4. Send a request
curl -sS -X POST http://127.0.0.1:3000/webhook \
  -H 'content-type: application/json' \
  -d '{"provider":"stripe","event":"invoice.paid"}'

# 5. Inspect and replay
flux logs --path /webhook --limit 20
flux trace <execution_id> --verbose
flux replay <execution_id> --diff
flux resume <execution_id>
```

## What to Look For

- trace shows full request payload and response/error
- replay output highlights changed JSON fields between original and replay
- resume continues from checkpointed call boundaries
