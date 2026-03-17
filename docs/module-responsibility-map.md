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

For `flux run --listen`, the split is the same after the CLI handoff: process bootstrapping stays in `runtime/src/main.rs`, while execution semantics stay in `runtime/src/deno_runtime.rs` and the runtime library.

Within the runtime library, responsibility splits again:

1. `runtime/src/http_runtime.rs` accepts HTTP traffic and converts it into execution requests.
2. `runtime/src/isolate_pool.rs` schedules work onto isolates and returns execution envelopes.
3. `runtime/src/server_client.rs` sends recorded execution data to `flux-server`.
4. `runtime/src/deno_runtime.rs` remains the only place that executes user JavaScript and exposes host behavior into that JavaScript environment.

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

### runtime/src/http_runtime.rs

Owns the HTTP-facing bridge into the runtime.

Safe changes here:

- define health and execution routes
- parse inbound HTTP requests and bodies
- filter sensitive headers before user-code dispatch
- translate `ExecutionResult` into HTTP responses
- wire execution recording after request completion

Do not put these here:

- JS runtime semantics
- replay policy
- isolate scheduling logic
- gRPC transport details

### runtime/src/isolate_pool.rs

Owns concurrency and scheduling for isolate execution.

Safe changes here:

- worker creation and teardown
- queue sizing, send timeouts, and result timeouts
- execution scheduling policy
- dispatch rules for one-shot execution versus server-mode requests
- execution result envelopes returned to callers

Do not put these here:

- HTTP routing
- CLI argument handling
- external recording transport
- new host-side JS APIs

### runtime/src/server_client.rs

Owns communication with `flux-server`.

Safe changes here:

- gRPC endpoint normalization
- auth metadata attachment
- protobuf request construction
- serialization of already-decided execution data

Do not put these here:

- logic for deciding what runtime behavior should be recorded
- HTTP server concerns
- isolate scheduling
- user-code execution

## Decision Rules

When a change request arrives, use this routing logic:

1. If it changes how `flux run` is invoked, validated, or forwarded, start in `cli/src/run.rs`.
2. If it changes how `flux-runtime` starts, resolves entries, or selects mode, start in `runtime/src/main.rs`.
3. If it changes JS execution semantics, host ops, replay behavior, or compatibility shims, start in `runtime/src/deno_runtime.rs`.
4. If it changes HTTP bridging, request shaping, or response shaping around execution, start in `runtime/src/http_runtime.rs`.
5. If it changes concurrency, worker lifecycle, queueing, or scheduling, start in `runtime/src/isolate_pool.rs`.
6. If it changes outbound recording transport or protobuf serialization, start in `runtime/src/server_client.rs`.

## Forbidden Cross-Layer Access

- `cli/src/run.rs` must not call runtime internals directly beyond launching `flux-runtime`.
- `runtime/src/main.rs` must not grow runtime semantics that belong in the runtime library.
- `runtime/src/deno_runtime.rs` must not depend on CLI modules or perform config discovery.
- `runtime/src/http_runtime.rs` must not become a second execution engine.
- `runtime/src/isolate_pool.rs` must not perform direct user-visible side effects outside Flux-controlled ops.
- `runtime/src/server_client.rs` must not decide execution semantics or replay policy.
- No single file should both execute user JavaScript and perform real external side effects outside the op boundary.

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