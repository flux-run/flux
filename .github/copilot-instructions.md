# Flux Workspace Guardrails

## Golden Rule

Flux owns all side effects.

JavaScript execution is untrusted and must not directly access external systems, time, or randomness without going through Flux-controlled ops.

## Determinism Rules

- All side effects must be observable, serializable, and replayable.
- If a side effect cannot be deterministically recorded and replayed, it must not be exposed to user code.
- Prefer synchronous, fully buffered boundaries for side effects.
- Do not introduce streaming or partial-consumption APIs unless they are fully deterministic and replay-safe.

## Product Scope

- Strengthen Flux as a deterministic execution engine for backend code.
- Preserve Flux-owned fetch, time, randomness, logging, and request dispatch boundaries.
- Treat Web Platform Tests and node-core tests as compatibility measurement tools, not automatic pass targets.
- Do not treat unsupported browser-only or Node-only behavior as a product bug unless it blocks the backend execution model Flux actually supports.
- Do not reintroduce broad Deno web or Node extension surfaces unless the change is explicitly required and consistent with Flux's recording model.

## Module Responsibility Map

### runtime/src/deno_runtime.rs

Owns the deterministic host/runtime boundary.

- Keep Flux-controlled ops here: fetch, time, randomness, logging, URL parsing, and server dispatch.
- Keep the minimal JS bootstrap here when host-owned behavior must be projected into user code.
- Keep ESM and TypeScript module loading behavior here when it is part of runtime execution semantics.
- Do not move CLI concerns, process orchestration, or config loading into this file.
- Do not expose new side effects here unless they satisfy the determinism rules above.

### runtime/src/main.rs

Owns flux-runtime process bootstrapping and mode selection.

- Parse runtime flags.
- Validate and resolve the entry file.
- Choose between script mode and HTTP serving mode.
- Prepare the runtime artifact and hand execution off to the runtime library.
- Do not implement web-platform behavior or compatibility shims here.

### cli/src/run.rs

Owns the flux run CLI contract.

- Validate CLI inputs.
- Discover the workspace root and runtime binary.
- Translate CLI arguments into the flux-runtime invocation contract.
- Keep this file focused on process handoff.
- Do not duplicate runtime semantics or implement host APIs here.

## Change Heuristics

- Prefer the smallest change that preserves the existing deterministic design.
- Fix problems at the ownership boundary where they originate instead of adding compensating logic in another layer.
- If a change touches compatibility behavior, first decide whether it is a real runtime gap, a harness gap, or an unsupported-by-design case.
- When in doubt, preserve Flux identity over expanding surface area.