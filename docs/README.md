# Flux Documentation

This documentation set is rebuilt for the current Flux CLI and runtime architecture.

## Start Here

- [quickstart.md](quickstart.md) — first run in under 10 minutes
- [bundled-artifacts.md](bundled-artifacts.md) — official v1 developer path: build first, then run
- [cli.md](cli.md) — full command surface and usage patterns
- [production-debugging.md](production-debugging.md) — incident workflow
- [concepts.md](concepts.md) — execution-record mental model
- [SPEC.md](SPEC.md) — scope and product guarantees

## Architecture

- [single-binary-architecture.md](single-binary-architecture.md) — current three-binary model and responsibilities
- [module-responsibility-map.md](module-responsibility-map.md) — file-level ownership for CLI handoff, runtime bootstrapping, and deterministic execution
- [execution-lifecycle.md](execution-lifecycle.md) — end-to-end flow for run, exec, serve, trace, replay, and resume
- [checkpoint-contract.md](checkpoint-contract.md) — checkpoint schema, replay guarantees, and resume semantics
- [failure-spec-concurrent-duplicate-requests.md](failure-spec-concurrent-duplicate-requests.md) — Case 1 failure contract for two recorded executions racing past coordination into one durable outcome
- [failure-spec-durable-write-before-checkpoint.md](failure-spec-durable-write-before-checkpoint.md) — Case 4 failure contract for durable write success before checkpoint capture
- [runtime/redis.md](runtime/redis.md) — Redis boundary contract, supported surface, and replay rules
- [api.md](api.md) — operator API role
- [api-reference.md](api-reference.md) — current exposed RPC/command mapping

## Examples

- [examples/hello-http.md](examples/hello-http.md) — minimal request/trace loop
- [examples/hono-bundled.md](examples/hono-bundled.md) — framework path with `flux build` + `npm:hono`
- [examples/drizzle-bundled.md](examples/drizzle-bundled.md) — direct `drizzle-orm/node-postgres` + `pg` over local `node_modules`
- [../examples/crud_app/README.md](../examples/crud_app/README.md) — larger CRUD sample using Hono + Drizzle with a Flux-buildable module graph
- [../examples/idempotency/README.md](../examples/idempotency/README.md) — Redis-backed idempotency keys with Postgres side-effect suppression and replay
- [../examples/webhook_dedup/README.md](../examples/webhook_dedup/README.md) — Redis-backed webhook deduplication with durable event recording and replay
- [examples/webhook-worker.md](examples/webhook-worker.md) — webhook intake + replay workflow
- [examples/exec-smoke.md](examples/exec-smoke.md) — one-off local execution sanity check
- [examples/drizzle-node-postgres.md](examples/drizzle-node-postgres.md) — `pg` compatibility surface and parser behavior for Drizzle-style apps
