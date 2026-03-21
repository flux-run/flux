# Failure Spec 04: Durable Write Before Checkpoint Capture

This document defines the first adversarial failure specification for Flux.

It is intentionally narrow. The goal is not to describe every bad thing that can happen. The goal is to pin down the one failure that most easily makes a replay system dishonest:

- the durable side effect succeeds
- the runtime dies before Flux records that fact

In this world, Postgres knows the truth and Flux does not.

If Flux handles this case correctly, the rest of the failure matrix becomes much easier to trust.

Status:

- specified
- executable via the `idempotency-crash-before-checkpoint` integration suite

## Why This Case Comes First

This is the sharpest split-brain scenario available in the current model.

The idempotency example in [../examples/idempotency/main_flux.ts](../examples/idempotency/main_flux.ts) uses:

- Redis for coordination
- Postgres for durable truth
- Flux checkpoints for trace and replay evidence

That means the dangerous edge is not "request fails." It is:

- Postgres commit succeeds
- Redis has not yet been updated
- no checkpoint has been durably recorded

That is the exact world where Flux must refuse to lie.

## Scope

This spec is written against the idempotency flow defined in [../examples/idempotency/main_flux.ts](../examples/idempotency/main_flux.ts) and the table defined in [../examples/idempotency/init.sql](../examples/idempotency/init.sql).

It also depends on the current lifecycle described in [execution-lifecycle.md](execution-lifecycle.md) and the replay contract defined in [checkpoint-contract.md](checkpoint-contract.md).

## 1. Setup

Initial state:

- Postgres table `idempotent_orders` exists and contains no row for the target idempotency key
- Redis contains no key `idempotency:order-123`
- the idempotency example is running with recording enabled

Request under test:

```http
POST /orders
content-type: application/json
idempotency-key: order-123

{"sku":"flux-shirt","quantity":1}
```

Required environment:

- `FLUX_SERVICE_TOKEN=dev-service-token`
- `DATABASE_URL=postgres://...`
- `REDIS_URL=redis://...`
- `FLUXBASE_ALLOW_LOOPBACK_POSTGRES=1`
- `FLUXBASE_ALLOW_LOOPBACK_REDIS=1`

Expected precondition checks:

- `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = 'order-123'` returns `0`
- `GET idempotency:order-123` returns nil

## 2. Trigger

The failure injection point must be runtime-owned.

Inject a fatal process abort in [../runtime/src/deno_runtime.rs](../runtime/src/deno_runtime.rs) with the one-shot env flag `FLUX_CRASH_AFTER_POSTGRES_COMMIT_BEFORE_CHECKPOINT=1`, at this exact timing inside the write-query path used by the idempotency example:

1. `perform_postgres_query(...)` has already returned success
2. the Postgres write is already committed
3. `execution.checkpoints.push(...)` has not run yet
4. control has not returned to user JavaScript
5. [../runtime/src/http_runtime.rs](../runtime/src/http_runtime.rs) has not yet called [../runtime/src/server_client.rs](../runtime/src/server_client.rs) to persist the execution envelope

This is the precise split-brain moment:

- durable state has changed
- Redis has not been updated
- Flux has not durably recorded the boundary crossing

Anything later is a different spec.

The hook is one-shot per runtime process so the retry path can proceed normally after the first crash.

## 3. Expected Trace

For the crashed first attempt:

- there is no completed execution record stored in `flux-server`
- there is no persisted checkpoint stream for the committed Postgres write
- `flux trace` must not fabricate a partial execution that was never recorded

This absence is part of the contract.

For the subsequent successful retry:

- trace must show only the retry execution
- trace must reflect the retry's real boundaries in order
- trace must not imply that the first crashed attempt completed successfully

Minimum required evidence in the retry trace:

- Redis lookup for `idempotency:order-123`
- Postgres activity that converges on the already-written row
- Redis population for the canonical stored response

The exact number of Postgres checkpoints may vary with implementation details, but the trace must remain literal rather than interpretive.

## 4. Expected Replay Behavior

The crashed first attempt is not replayable, because no execution record exists for it.

That is not a bug. That is the honest outcome.

Flux MUST preserve the following rules:

- replay must not invent missing checkpoints for the crashed attempt
- replay must not silently fall back to live execution to reconstruct that missing history
- replay is only valid for executions that were actually recorded

For the subsequent successful retry:

- replay must use only the retry execution's recorded checkpoints
- replay must not consult live Redis or live Postgres to fill historical gaps
- replay of the retry must return the retry's recorded result without duplicating durable state

This case therefore proves two different claims:

- missing history stays missing
- retry still converges safely because durable truth wins

## 5. Expected Durable State

After the first crashed attempt and before the retry:

- `idempotent_orders` contains exactly one row for `order-123`
- the row payload matches `sku = 'flux-shirt'` and `quantity = 1`
- Redis may still have no `idempotency:order-123` key

After the retry completes:

- `idempotent_orders` still contains exactly one row for `order-123`
- the logical order identity is unchanged
- Redis contains a canonical stored response for `idempotency:order-123`

Postgres is the durable authority in this spec. Redis is coordination state only.

## 6. Forbidden Outcomes

The following outcomes are illegal:

- duplicate row inserted into `idempotent_orders`
- second logical order ID created for the same idempotency key
- successful replay of the crashed first attempt without a recorded execution
- silent fallback to live execution while claiming replay semantics
- trace output that suggests the crashed attempt completed when it did not
- retry path that ignores the existing durable row and behaves as fresh work
- divergence between durable state and the behavior claimed by trace or replay

## 7. Invariant Check

This spec only passes if all four layers stay aligned:

- implementation: the failure was injected exactly between durable commit and checkpoint capture
- behavior: only one logical order exists after crash plus retry
- explanation: [checkpoint-contract.md](checkpoint-contract.md) and [../examples/idempotency/README.md](../examples/idempotency/README.md) still describe what actually happened
- evidence: durable state, trace, and replay output agree without fabrication

In shorthand:

```text
implementation = behavior = explanation = evidence
```

## What This Spec Proves

If Flux passes this spec, it proves a stronger claim than "replay works."

It proves:

- Flux does not pretend missing history exists
- retry after a partial crash can still converge safely
- durable truth is allowed to outrank coordination state
- observability remains honest even when history is incomplete

That is the minimum bar for calling the model trustworthy.