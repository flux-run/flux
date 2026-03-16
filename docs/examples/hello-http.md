# Example: Hello HTTP

A minimal end-to-end example to verify the local Flux loop.

## Goal

- start server/runtime
- send one request
- inspect execution details

## Steps

```bash
flux server start --database-url postgres://localhost:5432/postgres
flux init
flux serve index.ts
curl -sS -X POST http://127.0.0.1:3000/index \
  -H 'content-type: application/json' \
  -d '{"name":"world"}'
flux logs --limit 10
flux trace <execution_id> --verbose
flux why <execution_id>
```

## What to Look For

- the request appears in `flux logs`
- `flux trace --verbose` includes request and response bodies
- `flux why` returns an actionable diagnosis summary
