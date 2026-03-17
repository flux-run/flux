# Execution Lifecycle

This document describes how a Flux execution moves through the system today.

It is an operational map, not an aspirational design. The goal is to answer two questions quickly:

1. Where does a given piece of behavior happen?
2. Where should debugging start when that behavior looks wrong?

## Core Invariant

Flux owns all side effects.

User JavaScript runs inside the runtime, but external effects only cross the boundary through Flux-controlled ops and runtime-owned transport layers.

That is what makes recording, tracing, replay, and resume possible.

## Main Flows

Flux has four execution-related flows today:

1. `flux run`: local script execution through `flux-runtime` in script mode
2. `flux exec`: one-off HTTP-style execution against a temporary runtime
3. `flux serve`: long-running runtime serving requests and recording executions
4. `flux replay` / `flux resume`: server-driven re-evaluation of previously recorded executions

## Flow 1: `flux run`

This is the plain script path.

1. [cli/src/run.rs](cli/src/run.rs) validates the entry path and input JSON.
2. [cli/src/run.rs](cli/src/run.rs) finds the workspace root and `flux-runtime` binary.
3. [cli/src/run.rs](cli/src/run.rs) launches `flux-runtime` with `--script-mode` and `--script-input`.
4. [runtime/src/main.rs](runtime/src/main.rs) validates the entry, resolves bundled output from `flux.json` if present, and selects script mode.
5. [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs) creates a `JsIsolate` using the module loader and Flux bootstrap.
6. [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs) evaluates the entry module, detects whether a default handler exists, and either:
   - invokes the exported handler with the provided input, or
   - drains top-level async work and exits
7. If the script uses Flux-owned ops like `fetch`, `Date.now`, `Math.random`, or logging, those stay inside [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs).

### Debug Start Points

- CLI argument or process-launch issue: [cli/src/run.rs](cli/src/run.rs)
- entry resolution or mode selection issue: [runtime/src/main.rs](runtime/src/main.rs)
- JS execution, ESM, TS transpile, or host-op issue: [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)

## Flow 2: `flux exec`

This is the one-off recorded execution path.

1. [cli/src/exec.rs](cli/src/exec.rs) resolves auth and validates input.
2. [cli/src/exec.rs](cli/src/exec.rs) finds a free local port and starts a temporary `flux-runtime` process.
3. [runtime/src/main.rs](runtime/src/main.rs) boots the runtime in HTTP-serving mode.
4. [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs) exposes `/health` and `/:route`.
5. [cli/src/exec.rs](cli/src/exec.rs) waits for `/health`, then POSTs the input payload to the route.
6. [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs) turns that request into an `ExecutionContext` and passes it to [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs).
7. [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs) selects a worker and dispatches into [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs).
8. [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs) runs the handler, records checkpoints and logs, and returns a `JsExecutionOutput`.
9. [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs) wraps that into an `ExecutionResult`.
10. [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs) records the execution through [runtime/src/server_client.rs](runtime/src/server_client.rs).
11. [runtime/src/server_client.rs](runtime/src/server_client.rs) sends the execution envelope to `flux-server` over gRPC.
12. [server/src/grpc.rs](server/src/grpc.rs) persists the execution row and checkpoint rows.
13. [cli/src/exec.rs](cli/src/exec.rs) fetches the trace back from the server and prints it.

### Debug Start Points

- temporary runtime boot or health wait issue: [cli/src/exec.rs](cli/src/exec.rs)
- inbound request parsing or response shaping issue: [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)
- concurrency, queueing, or worker timeout issue: [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs)
- checkpoint or host-op issue: [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)
- recording transport or auth metadata issue: [runtime/src/server_client.rs](runtime/src/server_client.rs)
- persistence or trace retrieval issue: [server/src/grpc.rs](server/src/grpc.rs)

## Flow 3: `flux serve`

This is the long-running production-style runtime path.

1. The CLI starts `flux-runtime` in serve mode.
2. [runtime/src/main.rs](runtime/src/main.rs) validates the entry, prepares the artifact, and calls `run_http_runtime`.
3. [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs) builds an `IsolatePool` and checks whether the entry is:
   - one-shot handler mode, or
   - server mode via `Deno.serve`
4. For one-shot handler mode:
   - `POST /:route` enters [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)
   - execution is scheduled through [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs)
   - user JS runs inside [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)
   - the execution is optionally recorded through [runtime/src/server_client.rs](runtime/src/server_client.rs)
5. For server mode:
   - any unmatched path enters [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)
   - HTTP request state is converted to a `NetRequest`
   - [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs) dispatches the request into a long-lived isolate
   - [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs) routes it through the registered `Deno.serve` handler
   - the returned `NetResponse` is translated back into an Axum response in [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)

### Debug Start Points

- route mismatch, header filtering, or body shaping issue: [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)
- worker contention or lifecycle issue: [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs)
- `Deno.serve` bridging or JS semantics issue: [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)

## Flow 4: Trace, Replay, and Resume

These flows start from stored execution records in `flux-server`.

### Trace

1. [cli/src/trace.rs](cli/src/trace.rs) requests an execution by ID.
2. [server/src/grpc.rs](server/src/grpc.rs) loads the execution row and ordered checkpoints.
3. [cli/src/trace.rs](cli/src/trace.rs) renders the stored request, response, and checkpoints.

Trace is read-only. If trace output looks wrong, the bug is usually in:

- recording shape in [runtime/src/server_client.rs](runtime/src/server_client.rs)
- persistence shape in [server/src/grpc.rs](server/src/grpc.rs)
- original checkpoint generation in [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)

### Replay

Replay is currently server-driven, not in-process runtime replay.

1. [cli/src/replay.rs](cli/src/replay.rs) asks `flux-server` to replay an execution.
2. [server/src/grpc.rs](server/src/grpc.rs) loads the original execution and checkpoints.
3. For each checkpoint from `from_index` onward, the server either:
   - reuses the recorded response, or
   - makes a live HTTP call when `--commit` is requested for HTTP checkpoints
4. [server/src/grpc.rs](server/src/grpc.rs) stores a new replay execution and new replay checkpoint rows.
5. [cli/src/replay.rs](cli/src/replay.rs) prints the replay steps and optional diff against the original execution.

This is an important current limitation: replay does not yet re-enter [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs) through a direct in-process isolate call. That distinction matters when debugging replay behavior.

### Resume

1. [server/src/grpc.rs](server/src/grpc.rs) loads the source execution and all checkpoints.
2. Checkpoints before `from_index` stay recorded.
3. HTTP checkpoints at or after `from_index` are executed live by the server.
4. A new resumed execution and checkpoint set are stored.

Resume is therefore a checkpoint-level continuation tool at the server layer in the current architecture.

## Side-Effect Boundary by Stage

This is the invariant to preserve during future changes:

- user JavaScript executes only inside [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)
- real external recording transport happens in [runtime/src/server_client.rs](runtime/src/server_client.rs)
- inbound HTTP bridging happens in [runtime/src/http_runtime.rs](runtime/src/http_runtime.rs)
- scheduling happens in [runtime/src/isolate_pool.rs](runtime/src/isolate_pool.rs)
- persistence happens in [server/src/grpc.rs](server/src/grpc.rs) and [server/src/core.rs](server/src/core.rs)

No single file should collapse those concerns into one layer.

## Failure Classification Guidance

When a lifecycle issue appears, classify it before fixing it:

1. CLI contract issue: bad arguments, bad process handoff, bad user-facing invocation
2. Runtime boot issue: entry resolution, mode selection, or artifact preparation
3. Execution semantics issue: JS evaluation, compatibility shim, or host op behavior
4. Scheduling issue: queueing, timeout, or worker lifecycle
5. HTTP bridge issue: request shaping, response shaping, or header filtering
6. Recording transport issue: gRPC connection, auth metadata, serialization
7. Persistence issue: database write, trace lookup, replay storage, resume storage
8. Unsupported-by-design behavior: browser-only or Node-only expectations outside Flux scope

## Review Questions

Use these questions before changing the lifecycle:

1. Does this change introduce a new side effect?
2. If yes, where is it recorded?
3. Can it be replayed deterministically?
4. Does this change belong in the layer being edited?
5. Does it widen replay or resume semantics without making the boundary clearer?