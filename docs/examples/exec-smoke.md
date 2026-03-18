# Example: Exec Smoke Test

A shortest-path local sanity check using `flux exec`.

## Goal

- execute a JS file once without keeping the runtime running
- print output immediately
- capture an execution record for follow-up debugging

## Steps

```bash
# 1. Start the server
flux server start --database-url postgres://postgres:postgres@localhost:5432/flux

# 2. One-time auth setup
flux init

# 3. Run a one-off execution
flux exec examples/hello.js --input '{"health":"ok"}'
```

## Optional Follow-up

```bash
flux logs --limit 5
flux trace <execution_id> --verbose
flux why <execution_id>
```

## What to Look For

- `flux exec` exits cleanly with function output printed
- a new execution appears in `flux logs`
- `flux trace` and `flux why` work on that execution ID
