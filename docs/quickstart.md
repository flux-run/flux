# Quickstart

This quickstart shows what happens when you start building with Flux.

## 1. Build The CLI

From the repository root:

```bash
cargo build -p cli
```

The CLI binary is `target/debug/flux`.

## 2. Initialize A Project

```bash
target/debug/flux init my-app
cd my-app
```

The scaffold gives you a complete starting point:

- `flux.toml`
- `functions/`
- `schemas/`
- `middleware/`
- `queues/`
- `.env.example`
- local `.flux/` state for generated files and dev metadata

## 3. Start The Local Runtime

Flux requires a Postgres database. Start one with Docker:

```bash
docker run -p 5432:5432 -e POSTGRES_PASSWORD=flux postgres:16
export DATABASE_URL=postgres://postgres:flux@localhost/flux
```

Then start the dev server:

```bash
target/debug/flux dev
```

`flux dev` gives you:

- one command starts the stack (DATABASE_URL must be set)
- framework schema is applied automatically
- the operator API and dashboard are reachable
- the CLI prints the next commands you are likely to need

## 4. Create A Function

```bash
target/debug/flux function create create_user
```

Flux scaffolds a function that is ready to edit immediately.

## 5. Invoke The System

```bash
target/debug/flux invoke create_user --gateway --payload '{"email":"user@example.com"}'
```

The `--gateway` path is the most representative local path because it includes:

- routing
- middleware
- auth and validation hooks
- tracing and request IDs

## 6. Inspect The Execution Record

```bash
target/debug/flux trace
target/debug/flux trace <request_id>
target/debug/flux why <request_id>
```

After one request, the system already feels different from a logs-first stack:

- you have one request record
- you can inspect spans without stitching systems together manually
- you can see which code version ran
- you can connect the request to database mutations and downstream work

## 7. Evolve The Database

Update your schema or migration files, then apply them:

```bash
target/debug/flux db push
```

Flux treats the database as part of the execution model, not as a separate debugging blind spot.

## 8. Deploy

```bash
target/debug/flux deploy
```

The deployment loop:

- detects what changed
- bundles and uploads code
- records the deployment
- attaches deploy metadata to future executions

## 9. Debug A Real Incident

The real product loop starts after the first failure:

```bash
target/debug/flux errors
target/debug/flux debug
target/debug/flux why <request_id>
target/debug/flux incident replay --request-id <request_id>
target/debug/flux trace diff <original_id> <replay_id>
```

## What This Quickstart Demonstrates

A new developer can:

1. start a project without reading internal docs
2. create and invoke one function without port confusion
3. inspect one execution record immediately
4. understand one failure with `flux why`
5. feel that debugging is materially better than their existing stack
