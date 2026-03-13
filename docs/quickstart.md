# Quickstart

This quickstart describes the intended first-run experience for Flux.

> Flux is still in development. Treat this document as the target 0.1 beta workflow and the product contract the codebase is moving toward.

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

The scaffold should give you a complete starting point:

- `flux.toml`
- `functions/`
- `schemas/`
- `middleware/`
- `queues/`
- `agents/`
- `.env.example`
- local `.flux/` state for generated files and dev metadata

## 3. Start The Local Runtime

```bash
target/debug/flux dev
```

The target local experience is:

- one command starts the stack
- Postgres is bootstrapped or connected automatically
- framework and project schema are applied
- the operator API and dashboard are reachable
- the CLI prints the next commands you are likely to need

## 4. Create A Function

```bash
target/debug/flux function create create_user
```

Flux should scaffold a function that is ready to edit immediately.

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

After one request, the system should already feel different from a logs-first stack:

- you have one request record
- you can inspect spans without stitching systems together manually
- you can see which code version ran
- you can connect the request to database mutations and downstream work

## 7. Evolve The Database

Update your schema or migration files, then apply them:

```bash
target/debug/flux db push
```

Flux is meant to treat the database as part of the execution model, not as a separate debugging blind spot.

## 8. Deploy

```bash
target/debug/flux deploy
```

The intended deployment loop is:

- detect what changed
- bundle and upload code
- record the deployment
- attach deploy metadata to future executions

## 9. Debug A Real Incident

The real product loop starts after the first failure:

```bash
target/debug/flux errors
target/debug/flux debug
target/debug/flux why <request_id>
target/debug/flux incident replay --request-id <request_id>
target/debug/flux trace diff <original_id> <replay_id>
```

## What Quickstart Must Prove

Flux is ready for serious beta testing when a new developer can:

1. start a project without reading internal docs
2. create and invoke one function without port confusion
3. inspect one execution record immediately
4. understand one failure with `flux why`
5. feel that debugging is materially better than their existing stack
