# Example: Exec Smoke Test

A shortest-path local sanity check using `flux exec`.

## Goal

- execute a function file once without keeping runtime running
- print output immediately
- capture an execution record for follow-up debugging

## Steps

```bash
flux server start --database-url postgres://localhost:5432/postgres
flux init
flux exec index.ts --payload '{"health":"ok"}'
```

## Optional Follow-up

```bash
flux logs --limit 5
flux trace <execution_id> --verbose
flux why <execution_id>
```

## What to Look For

- `flux exec` exits cleanly with function output
- a new execution appears in `flux logs`
- trace/why commands work on that execution ID
