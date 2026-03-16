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
target/debug/flux server start --database-url postgres://localhost:5432/postgres
```

## 3) Initialize Auth Once

```bash
target/debug/flux init
```

After this, commands work without repeating `--url` and `--token`.

## 4) Start Runtime

```bash
target/debug/flux serve index.ts
```

Runtime endpoint:

```bash
POST http://127.0.0.1:3000/index
```

## 5) Run One-Off Execution

```bash
target/debug/flux exec index.ts --payload '{"email":"user@example.com"}'
```

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
