# CLI

Flux CLI (`flux`) manages the build pipeline, server process, runtime execution, and debugging.

## Build Pipeline

### `flux build` — analyse and bundle for production

```bash
flux build                        # analyses index.ts, writes flux.json
flux build server.ts              # explicit entry
flux build server.ts --no-bundle  # skip esbuild, just write flux.json
flux build server.ts --no-minify  # bundle without minification
```

`flux build` reads the entry file, detects which runtime features it needs
(fetch, WebSocket, node:*, fs, net, os, subprocess), and writes **`flux.json`**
to the same directory. If `esbuild` is on `PATH` it also produces `.flux/bundle.js`.

### `flux dev` — development server with hot reload

```bash
flux dev                          # watch index.ts, restart on any .ts/.js change
flux dev server.ts
flux dev server.ts --port 8080
flux dev server.ts --poll-ms 200  # faster poll interval
flux dev server.ts --watch-dir ./src
```

On each file change `flux dev` re-analyses imports, writes a fresh `flux.json`,
kills the running runtime, and respawns it. No external file-watcher dependency
— uses mtime polling every 500 ms by default.

### `flux.json` manifest

`flux build` (and `flux dev` on each restart) write a `flux.json` file that
tells the runtime which capability modules to load:

```json
{
  "flux_version": "0.2",
  "entry": "server.ts",
  "code_hash": "a1b2c3d4e5f6",
  "built_at": "2026-01-01T00:00:00Z",
  "runtime_features": ["web", "fetch", "crypto", "node", "fs"],
  "bundled": ".flux/bundle.js",
  "minified": true
}
```

| Feature | Activated by |
|---|---|
| `web` | always (TextEncoder, streams, AbortController) |
| `fetch` | always (fetch, Headers, Request, Response) |
| `crypto` | always (SubtleCrypto, randomUUID) |
| `websocket` | `WebSocket` in source |
| `node` | `require(`, `from 'node:…'`, bare npm imports |
| `fs` | `node:fs`, `Deno.readFile`, `Deno.writeFile` |
| `net` | `node:net`, `Deno.connect`, `Deno.listen` |
| `os` | `process.env`, `Deno.env`, `node:os` |
| `process` | `Deno.Command`, `node:child_process` |

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

# production: serve a pre-built entry (requires flux.json + running server)
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
flux resume <execution_id>
flux resume <execution_id> --from 2
```

`flux replay --commit --validate` turns live replay divergence into a loud failure: if a live HTTP checkpoint result differs from the recorded checkpoint result, replay is marked as an error instead of silently drifting.

When validated replay detects divergence, `flux` exits with code `2`. This makes replay validation usable in CI and automation.

`flux replay --explain` renders replay as an execution narrative: each checkpoint shows whether it came from recorded history or live execution, whether live execution validated successfully, and where the first divergence occurred.

## One-Off Local Run

```bash
flux run index.ts                         # run as a plain script
flux exec index.ts --input '{"amount":100}'
flux exec index.ts --input '{"amount":100}' --timeout-secs 30
```

## UX Conventions

- list views show short IDs (8 chars)
- detail views use full execution IDs
- status colors: `✓ ok` (green), `✗ error` (red), `⚠ slow` (yellow)
- after `flux init`, no repeated auth flags are required
