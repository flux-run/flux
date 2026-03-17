# Quickstart

## 1) Build CLI

```bash
cargo build -p cli
```

Binary path:

```bash
target/debug/flux
```

## 2) Start Flux Server

```bash
target/debug/flux server start --database-url postgres://localhost:5432/flux
```

The `--service-token` flag (or `INTERNAL_SERVICE_TOKEN` env var) sets the shared secret between server and runtime. Defaults to `dev-service-token` for local development.

## 3) Initialize Auth Once

```bash
target/debug/flux init
```

After this, commands work without repeating `--url` and `--token`.

## 4) Start Runtime

```bash
target/debug/flux run index.ts --listen
```

Runtime endpoint:

```bash
POST http://127.0.0.1:3000/index
```

## 5) Run One-Off Execution

```bash
target/debug/flux exec index.ts --input '{"email":"user@example.com"}'
```

For a focused smoke-test flow, see [examples/exec-smoke.md](examples/exec-smoke.md).

## 6) Inspect

```bash
target/debug/flux logs --limit 20
target/debug/flux trace <execution_id> --verbose
target/debug/flux why <execution_id>
```

## 7) Replay / Compare

```bash
target/debug/flux replay <execution_id> --diff
target/debug/flux resume <execution_id>
```

## 8) Health Checks

```bash
target/debug/flux ps
target/debug/flux status
```
