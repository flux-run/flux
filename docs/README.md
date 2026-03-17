# Flux Documentation

This documentation set is rebuilt for the current Flux CLI and runtime architecture.

## Start Here

- [quickstart.md](quickstart.md) — first run in under 10 minutes
- [cli.md](cli.md) — full command surface and usage patterns
- [production-debugging.md](production-debugging.md) — incident workflow
- [concepts.md](concepts.md) — execution-record mental model
- [SPEC.md](SPEC.md) — scope and product guarantees

## Architecture

- [single-binary-architecture.md](single-binary-architecture.md) — current three-binary model and responsibilities
- [module-responsibility-map.md](module-responsibility-map.md) — file-level ownership for CLI handoff, runtime bootstrapping, and deterministic execution
- [execution-lifecycle.md](execution-lifecycle.md) — end-to-end flow for run, exec, serve, trace, replay, and resume
- [checkpoint-contract.md](checkpoint-contract.md) — checkpoint schema, replay guarantees, and resume semantics
- [api.md](api.md) — operator API role
- [api-reference.md](api-reference.md) — current exposed RPC/command mapping

## Examples

- [examples/hello-http.md](examples/hello-http.md) — minimal request/trace loop
- [examples/webhook-worker.md](examples/webhook-worker.md) — webhook intake + replay workflow
- [examples/exec-smoke.md](examples/exec-smoke.md) — one-off local execution sanity check
