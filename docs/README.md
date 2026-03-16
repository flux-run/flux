# Flux Documentation

This directory is the public product and architecture guide for Flux.

Flux is a complete backend runtime built around deterministic debugging.

## Recommended Reading Order

1. [../README.md](../README.md)
2. [quickstart.md](quickstart.md)
3. [concepts.md](concepts.md)
4. [cli.md](cli.md)
5. [single-binary-architecture.md](single-binary-architecture.md)
6. [production-debugging.md](production-debugging.md)

## Product Docs

- [framework.md](framework.md) - what Flux is, what it includes, and how it feels
- [SPEC.md](SPEC.md) - product goals, principles, and user model
- [quickstart.md](quickstart.md) - first-run developer workflow
- [concepts.md](concepts.md) - core mental model and primitives
- [cli.md](cli.md) - command-line workflows and command philosophy

## Architecture Docs

- [single-binary-architecture.md](single-binary-architecture.md) - overall system architecture and deployment model
- [api.md](api.md) - operator-facing API responsibilities
- [runtime.md](runtime.md) - execution engine for user code
- [queue.md](queue.md) - background work and retries
- [observability.md](observability.md) - execution record, spans, logs, and debugging surfaces
- [storage.md](storage.md) - persisted state, bundles, caches, and retention
- [database-schema.md](database-schema.md) - logical schema layout
- [wasm-runtime.md](wasm-runtime.md) - WebAssembly and multi-language execution

## Product Narrative And Debugging Docs

- [production-debugging.md](production-debugging.md) - incident response workflow
- [flux-why-the-viral-command.md](flux-why-the-viral-command.md) - why `flux why` is the hero feature
- [git-for-backend-execution.md](git-for-backend-execution.md) - execution record as a version-control-like model
## Operational And Reference Docs

- [api-reference.md](api-reference.md) - route groups and API surface
- [FOLDER_STRUCTURE.md](FOLDER_STRUCTURE.md) - repo layout and generated project layout

## Examples

- [examples/todo-api.md](examples/todo-api.md)
- [examples/webhook-worker.md](examples/webhook-worker.md)
- [examples/ai-backend.md](examples/ai-backend.md)
