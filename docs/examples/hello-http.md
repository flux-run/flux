# Example: Hello HTTP

A minimal end-to-end example to verify the local Flux loop.

## Goal

- start server and runtime
- send one request
- inspect the execution record

## Steps

```bash
# 1. Start the gRPC server
flux server start --database-url postgres://postgres:postgres@localhost:5432/flux

# 2. One-time auth setup
flux init

# 3. Serve a JS entry file
flux serve examples/hello.js

# 4. Send a request
curl -sS -X POST http://127.0.0.1:3000/hello \
  -H 'content-type: application/json' \
  -d '{"name":"world"}'

# 5. Inspect
flux logs --limit 10
flux trace <execution_id> --verbose
flux why <execution_id>
```

## What to Look For

- the request appears in `flux logs`
- `flux trace --verbose` includes request and response bodies
- `flux why` returns an actionable diagnosis summary
