# Checkpoint Contract

This document defines the checkpoint primitive in Flux and the replay guarantees attached to it.

It separates three things that are easy to blur together:

1. the checkpoint shape Flux records today
2. the determinism contract Flux is trying to preserve
3. the current limitations of replay and resume in the existing three-binary architecture

## Why This Exists

Flux is not just a runtime. It is a system for understanding execution.

That depends on checkpoints being treated as a product primitive, not incidental debug data.

If checkpoint shape drifts, replay semantics drift with it.
If replay semantics drift, the system stops being trustworthy.

## Current Checkpoint Shape

Today, a checkpoint is the recorded representation of a boundary crossing observed during execution.

In the runtime, the core shape is defined by [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs):

- `call_index`
- `boundary`
- `url`
- `method`
- `request`
- `response`
- `duration_ms`

That shape is transported over gRPC through [shared/proto/internal_auth.proto](shared/proto/internal_auth.proto) as `CheckpointEntry` and persisted by [server/src/grpc.rs](server/src/grpc.rs) into the `flux.checkpoints` table created in [server/src/core.rs](server/src/core.rs).

## Storage Model Today

The persisted checkpoint row currently contains:

- `execution_id`
- `call_index`
- `boundary`
- `url`
- `method`
- `request`
- `response`
- `duration_ms`
- `created_at`

This means the durable identity of a checkpoint today is:

- execution-scoped
- ordered by `call_index`
- typed by `boundary`

It is not yet a globally addressable event object with its own first-class ID.

## Boundary Meaning

The `boundary` field is the semantic type of the checkpoint.

Examples:

- `http` for outbound HTTP calls
- future deterministic boundaries such as `time`, `random`, `db`, or other Flux-owned side effects

The boundary type answers one key question:

What kind of side effect or host interaction is being recorded here?

That field should stay small, explicit, and stable. Do not turn it into a catch-all bucket for arbitrary debug metadata.

## Request and Response Payloads

The `request` and `response` payloads are the serializable envelope of the boundary crossing.

For `http`, that typically means:

- request URL
- request method
- normalized headers
- request body
- response status
- response headers
- response body

The contract here is not “store whatever was convenient.”

The contract is:

- store enough input to explain the side effect
- store enough output to replay or inspect the side effect
- keep the shape deterministic and JSON-serializable

## Checkpoint Identity Rule

A checkpoint is uniquely identified by:

- the parent `execution_id`
- its ordered `call_index`

That means `call_index` must remain:

- monotonic within an execution
- deterministic for the same execution path
- stable enough that replay and resume can address boundaries by index

If a change makes checkpoint ordering ambiguous, it is a design problem, not just an implementation detail.

## Determinism Contract

Flux should preserve the following rule:

Given the same code, the same input, and the same recorded checkpoints consumed at the same boundary indices, execution should produce the same externally observed result.

In shorthand:

same code + same input + same checkpoints => same result

That is the system-level contract behind replayability.

## Important Qualification for Current Architecture

That full contract is only partially realized today.

In the current architecture:

- execution is runtime-driven in [runtime/src/deno_runtime.rs](runtime/src/deno_runtime.rs)
- replay and resume are server-driven in [server/src/grpc.rs](server/src/grpc.rs)

So today, Flux guarantees two different things depending on the path:

### Execution Guarantee

During runtime execution, Flux records the actual boundary envelopes observed by user JavaScript.

### Replay Guarantee Today

For non-commit replay, Flux re-simulates execution history from stored checkpoint state at the server layer.

That means current replay is best understood as:

- deterministic reconstruction of stored boundary outcomes
- not yet full in-isolate deterministic re-execution of user JavaScript

### Commit Replay and Resume Guarantee Today

When replay or resume performs live HTTP calls, the guarantee changes.

That path is not “same checkpoints => same result.”

It is instead:

- recorded history before the live boundary remains fixed
- the system continues from a known checkpoint boundary
- live side effects after that boundary may diverge

That distinction should stay explicit in both code and docs.

## Replay Contract

The replay contract for Flux should be written as two modes.

### Mode 1: Deterministic Replay

If replay consumes only recorded checkpoints and does not perform live side effects, Flux should preserve this invariant:

- the same checkpoint sequence yields the same replay result

This is the strongest replay guarantee and the one that matters for trust.

### Mode 2: Replay-Then-Continue

If replay or resume reuses recorded checkpoints up to boundary `N` and then resumes live execution, Flux should preserve this invariant:

- all boundaries before `N` are fixed by recorded state
- all boundaries after `N` are new execution history

That is continuation, not pure replay.

## Resume Contract

Resume should be defined as:

Replay until checkpoint `N`, then continue live execution from that point and record a new execution history.

In shorthand:

resume = replay prefix + live suffix

This is the right mental model for the current implementation in [server/src/grpc.rs](server/src/grpc.rs).

## Recommended Shape Evolution

If Flux wants a stricter long-term checkpoint primitive, the next additions should be deliberate.

Useful future fields:

- `op_type` or stabilized `boundary` taxonomy
- explicit `started_at` or boundary timestamp
- structured input and output envelopes by boundary type
- optional `parent_execution_id` or lineage fields for replayed and resumed executions
- explicit replay provenance showing whether a checkpoint was recorded, replayed, or executed live

These should only be added when they strengthen determinism or explainability. Do not add fields just because they seem generally useful.

## Review Rules

Before changing checkpoint shape, ask:

1. Does this field help explain a side effect boundary?
2. Does it make replay or resume more deterministic?
3. Can it be serialized and stored stably?
4. Will it remain meaningful across versions?
5. Does it belong in the checkpoint, or is it just incidental debug noise?

## Design Red Flags

Treat these as warning signs:

- checkpoint order stops being deterministic
- request or response payloads stop being fully serializable
- replay silently mixes recorded and live results without marking the difference
- checkpoint schema starts depending on UI needs instead of execution semantics
- a side effect happens without a corresponding checkpoint or other Flux-owned record

If any of those happen, Flux is drifting away from being a recorded runtime.