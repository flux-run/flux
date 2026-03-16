# CLI

Flux CLI (`flux`) manages server/runtime processes and execution debugging.

## Setup

```bash
# interactive: prompts for server URL and service token
flux init

# non-interactive auth
flux auth --url localhost:50051 --token <token>

# inspect or update saved config
flux config get
flux config get url
flux config get token
flux config set url localhost:50051
flux config set token <token>
```

## Process Management

```bash
flux server start --database-url postgres://...
flux server restart --database-url postgres://...

# serve a JS/TS entry file (requires server running)
flux serve index.ts
flux serve index.ts --host 0.0.0.0 --port 8080 --isolate-pool-size 8

flux ps
flux status
```

## Execution Commands

```bash
flux logs --limit 50
flux logs --status error --path /payment --since 1h --search stripe
flux tail
flux tail --project-id <id>
flux trace <execution_id> --verbose
flux why <execution_id>
```

## Replay / Recovery

```bash
flux replay <execution_id>
flux replay <execution_id> --from-index 1 --diff
flux resume <execution_id>
flux resume <execution_id> --from 2
```

## One-Off Local Run

```bash
flux exec index.ts --payload '{"amount":100}'
flux exec index.ts --payload '{"amount":100}' --timeout-secs 30
```

## UX Conventions

- list views show short IDs (8 chars)
- detail views use full execution IDs
- status colors: `✓ ok` (green), `✗ error` (red), `⚠ slow` (yellow)
- after `flux init`, no repeated auth flags are required
