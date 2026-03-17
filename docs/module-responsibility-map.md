# Module Responsibility Map

This map defines which file owns which part of Flux's execution path so changes stay localized and deterministic.

## Core Rule

Flux owns all side effects. User JavaScript only reaches external systems, time, randomness, and request/response boundaries through Flux-controlled ops.

That means side effects exposed to user code must be:

- observable
- serializable
- replayable

If a boundary cannot meet those constraints, it should not be added.

## Execution Flow

For `flux run`, responsibility flows in one direction:

1. `cli/src/run.rs` validates the CLI contract and launches `flux-runtime`.
2. `runtime/src/main.rs` resolves the entry, validates runtime mode, and starts the runtime process.
3. `runtime/src/deno_runtime.rs` hosts the embedded JS engine, module loading, deterministic ops, and compatibility shim.

For `flux serve`, the split is the same after the CLI handoff: process bootstrapping stays in `runtime/src/main.rs`, while execution semantics stay in `runtime/src/deno_runtime.rs` and the runtime library.

## File Ownership

### cli/src/run.rs

Owns the user-facing contract for `flux run`.

Safe changes here:

- add or refine `flux run` flags
- validate input before spawning the runtime
- change how the CLI finds `flux-runtime`
- adjust how the CLI forwards arguments to the runtime process

Do not put these here:

- JavaScript runtime semantics
- URL, fetch, timer, or replay behavior
- compatibility shims
- user-module evaluation logic

### runtime/src/main.rs

Owns the `flux-runtime` process entrypoint.

Safe changes here:

- parse runtime flags
- validate and canonicalize the entry path
- load `flux.json` when selecting bundled output
- choose script mode versus server mode
- call the right runtime library entrypoint

Do not put these here:

- JS platform behavior
- fetch interception details
- module shim logic
- replay bookkeeping

### runtime/src/deno_runtime.rs

Owns the embedded execution engine and the deterministic host boundary.

Safe changes here:

- add or refine Flux-controlled ops
- tighten SSRF, fetch, time, random, logging, or replay semantics
- improve ESM and TypeScript loading behavior
- extend minimal web-compatible shims when they help supported backend code
- fix runtime bugs surfaced by real compatibility probes

Do not put these here:

- CLI parsing or workspace discovery
- runtime process orchestration
- config file lookup
- broad browser or Node API expansion that bypasses Flux recording

## Decision Rules

When a change request arrives, use this routing logic:

1. If it changes how `flux run` is invoked, validated, or forwarded, start in `cli/src/run.rs`.
2. If it changes how `flux-runtime` starts, resolves entries, or selects mode, start in `runtime/src/main.rs`.
3. If it changes JS execution semantics, host ops, replay behavior, or compatibility shims, start in `runtime/src/deno_runtime.rs`.

## Compatibility Policy

- Use WPT and node-core suites to measure behavior and catch regressions.
- Do not chase full browser parity or Node parity.
- Classify failures before fixing them: runtime gap, harness gap, browser-only, or unsupported-by-design.
- Prefer fixes that improve backend-relevant execution over spec-corner completeness.

## Review Checklist

Before merging a change in this area, verify:

1. The edit landed in the file that actually owns the behavior.
2. Any new side effect remains observable, serializable, and replayable.
3. The change does not silently widen Flux into a generic browser or Node runtime.
4. Compatibility gains are measured without converting unsupported behavior into scope creep.