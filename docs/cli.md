# CLI

Flux CLI (`flux`) manages the build pipeline, server process, runtime execution, and debugging.

## Build Pipeline

### `flux build` — resolve and freeze an artifact for production

```bash
flux build              # resolves the configured entry and writes .flux/artifact.json
flux build server.ts    # explicit entry
```

`flux build` resolves the full ESM import graph from the entry, rejects unsupported imports (`node:*`, `require()`, bare package imports), freezes remote and `npm:` dependencies into a deterministic artifact, and writes **`.flux/artifact.json`** beside **`flux.json`**.

### `flux check` — compatibility analysis

```bash
flux check              # check the configured entry
flux check server.ts    # check an explicit entry
```

`flux check` performs static analysis first and reports:

- errors for `node:*` imports and `require()` usage
- warnings for unsupported globals and unsupported web APIs
- compatibility status for `npm:` dependencies

### `flux dev` — development server with hot reload

```bash
flux dev                          # watch index.ts, restart on any .ts/.js change
flux dev server.ts
flux dev server.ts --port 8080
flux dev server.ts --poll-ms 200  # faster poll interval
flux dev server.ts --watch-dir ./src
```

On each file change `flux dev` kills the running runtime and respawns it from source. It fingerprints the watched tree recursively, prioritizes correctness over isolate reuse, and reuses the existing `flux-runtime` binary instead of rebuilding it on every restart.

### `flux.json` project config

`flux init` scaffolds a stable project config:

```json
{
  "flux_version": "0.2",
  "entry": "./index.ts",
  "artifact": "./.flux/artifact.json"
}
```

`flux build` writes the deterministic module graph to `.flux/artifact.json`. `flux serve` only executes that built artifact.

## Setup

```bash
# scaffold a new runnable Flux project in the current directory
flux init

# migrate the old interactive auth flow explicitly
flux init --auth

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

# production: serve a pre-built entry (requires flux build first)
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
flux replay <execution_id> --commit --validate
flux replay <execution_id> --commit --validate --explain
flux replay <execution_id> --commit --validate --explain --ignore timestamp,requestId
flux resume <execution_id>
flux resume <execution_id> --from 2
```

`flux replay --commit --validate` turns live replay divergence into a loud failure: if a live HTTP checkpoint result differs from the recorded checkpoint result, replay is marked as an error instead of silently drifting.

When validated replay detects divergence, `flux` exits with code `2`. This makes replay validation usable in CI and automation.

`flux replay --explain` renders replay as an execution narrative: each checkpoint shows whether it came from recorded history or live execution, whether live execution validated successfully, and where the first divergence occurred.

`--ignore` is display-only: it hides matching diff paths or field names from replay output, but it does not change validation behavior or exit codes.

## One-Off Local Run

```bash
flux run index.ts                         # run as a plain script
flux run index.ts --listen               # run as a long-lived HTTP runtime
flux run index.ts --url http://127.0.0.1:50051   # record the script execution
flux exec index.ts --input '{"amount":100}'
flux exec index.ts --input '{"amount":100}' --timeout-secs 30
```

When `flux run` is given a Flux server URL and token, the runtime records the execution and prints an `execution_id:` line before the final JSON payload. That ID can be used with `flux trace`, `flux why`, and `flux replay`.

## UX Conventions

- list views show short IDs (8 chars)
- detail views use full execution IDs
- status colors: `✓ ok` (green), `✗ error` (red), `⚠ slow` (yellow)
- after `flux init --auth` or `flux auth`, no repeated auth flags are required
