# Flux Documentation

This directory explains the product Flux is intended to become: a complete backend runtime built around deterministic debugging.

The rest of the docs describe the target 0.1 beta shape. When implementation still lags the product story, [implementation-status.md](implementation-status.md) is the place that should say so explicitly.

## Recommended Reading Order

1. [../README.md](../README.md)
2. [quickstart.md](quickstart.md)
3. [concepts.md](concepts.md)
4. [cli.md](cli.md)
5. [single-binary-architecture.md](single-binary-architecture.md)
6. [production-debugging.md](production-debugging.md)

## Product Docs

- [framework.md](framework.md) - what Flux is, what it includes, and how it should feel
- [SPEC.md](SPEC.md) - product goals, non-goals, user model, and 0.1 beta target
- [quickstart.md](quickstart.md) - first-run developer workflow
- [concepts.md](concepts.md) - core mental model and primitives
- [cli.md](cli.md) - command-line workflows and command philosophy
- [implementation-status.md](implementation-status.md) - target state versus current repo state

## Architecture Docs

- [single-binary-architecture.md](single-binary-architecture.md) - overall system architecture and deployment model
- [api.md](api.md) - operator-facing API responsibilities
- [gateway.md](gateway.md) - ingress routing and policy enforcement
- [runtime.md](runtime.md) - execution engine for user code
- [data-engine.md](data-engine.md) - database execution and mutation recording
- [queue.md](queue.md) - background work and retries
- [observability.md](observability.md) - execution record, spans, logs, and debugging surfaces
- [storage.md](storage.md) - persisted state, bundles, caches, and retention
- [database-schema.md](database-schema.md) - logical schema layout
- [wasm-runtime.md](wasm-runtime.md) - target-state WebAssembly story

## Product Narrative And Debugging Docs

- [production-debugging.md](production-debugging.md) - incident response workflow
- [flux-why-the-viral-command.md](flux-why-the-viral-command.md) - why `flux why` is the hero feature
- [git-for-backend-execution.md](git-for-backend-execution.md) - execution record as a version-control-like model
- [workflow-to-agents-migration.md](workflow-to-agents-migration.md) - how orchestration fits into the broader product

## Operational And Reference Docs

- [api-reference.md](api-reference.md) - target-state route groups and API surface
- [gateway-production-checklist.md](gateway-production-checklist.md) - production hardening checklist
- [FOLDER_STRUCTURE.md](FOLDER_STRUCTURE.md) - repo layout and generated project layout
- [cli-rewrite-plan.md](cli-rewrite-plan.md) - CLI design roadmap for keeping the product loop simple

## Examples

- [examples/todo-api.md](examples/todo-api.md)
- [examples/webhook-worker.md](examples/webhook-worker.md)
- [examples/ai-backend.md](examples/ai-backend.md)
