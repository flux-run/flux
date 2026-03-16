# CLI

Flux CLI (`flux`) manages server/runtime processes and execution debugging.

## Setup

```bash
flux init
flux auth --url localhost:50051 --token <token>
flux config set url localhost:50051
flux config get token
```

## Process Management

```bash
flux server start --database-url postgres://...
flux server restart
flux serve index.ts
flux ps
flux status
```

## Execution Commands

```bash
flux logs --limit 50
flux logs --status error --path /payment --since 1h --search stripe
flux tail
flux trace <execution_id> --verbose
flux why <execution_id>
```

## Replay / Recovery

```bash
flux replay <execution_id>
flux replay <execution_id> --from-index 1 --diff
flux resume <execution_id>
```

## One-Off Local Run

```bash
flux exec index.ts --payload '{"amount":100}'
```

## UX Conventions

- list views show short IDs (8 chars)
- detail views use full execution IDs
- status colors: `✓ ok` (green), `✗ error` (red), `⚠ slow` (yellow)
- after `flux init`, no repeated auth flags are required
