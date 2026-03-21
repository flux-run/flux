# Failure Spec 01: Concurrent Duplicate Requests

This document defines the contention-side adversarial failure specification for Flux.

It is the complement to [failure-spec-durable-write-before-checkpoint.md](failure-spec-durable-write-before-checkpoint.md).

That spec protects the system when history disappears.
This one protects the system when history duplicates.

In this world, Flux must preserve two truths at once:

- both executions really happened
- only one durable effect is allowed to survive

If Flux handles this case correctly, it proves that correctness does not depend on timing or coordination state.

Status:

- specified
- not yet executable in the integration harness

## Why This Case Matters

The idempotency example in [../examples/idempotency/main_flux.ts](../examples/idempotency/main_flux.ts) uses:

- Redis for coordination
- Postgres for durable truth
- Flux checkpoints for trace and replay evidence

The dangerous race is not "duplicate request arrives later." It is:

- request A misses Redis
- request B misses Redis
- both continue before coordination state exists

That is the exact world where Redis must stop being a correctness authority.

If the system is still correct there, the architecture is real.

## Scope

This spec is written against the idempotency flow defined in [../examples/idempotency/main_flux.ts](../examples/idempotency/main_flux.ts) and the table defined in [../examples/idempotency/init.sql](../examples/idempotency/init.sql).

It also depends on the current lifecycle described in [execution-lifecycle.md](execution-lifecycle.md) and the replay contract defined in [checkpoint-contract.md](checkpoint-contract.md).

## 1. Setup

Initial state:

- Postgres table `idempotent_orders` exists and contains no row for the target idempotency key
- Redis contains no key `idempotency:abc123`
- the idempotency example is running with recording enabled
- the unique constraint on `idempotency_key` is active

Requests under test:

```http
POST /orders
content-type: application/json
idempotency-key: abc123

{"sku":"flux-shirt","quantity":1}
```

Two identical requests must be issued concurrently.

Required environment:

- `FLUX_SERVICE_TOKEN=dev-service-token`
- `DATABASE_URL=postgres://...`
- `REDIS_URL=redis://...`
- `FLUXBASE_ALLOW_LOOPBACK_POSTGRES=1`
- `FLUXBASE_ALLOW_LOOPBACK_REDIS=1`

Expected precondition checks:

- `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = 'abc123'` returns `0`
- `GET idempotency:abc123` returns nil

## 2. Barrier Point

The synchronization point is semantic, not transport-level.

Inject an app-level one-shot barrier with the env flag `FLUX_BARRIER_AFTER_REDIS_MISS_BEFORE_POSTGRES=1` at this exact timing in the idempotency flow:

1. `REDIS GET idempotency:abc123` has already returned null
2. control has not yet entered the Postgres write path
3. both requests must arrive at the barrier before either request may proceed

This is the precise contention moment:

- coordination state is absent for both requests
- both executions are still live
- correctness must now come from durable arbitration and response convergence

Anything earlier tests Redis coordination. Anything later weakens the race.

## 3. Trigger

1. send request A and request B concurrently
2. both observe `REDIS GET -> null`
3. both pause at the barrier
4. release both simultaneously

After release, both requests proceed into the durable path.

## 4. Expected Execution Records

Both executions must remain visible.

Required outcomes:

- execution A exists
- execution B exists
- neither execution is merged into the other
- neither execution is suppressed from trace

This is a truth requirement, not an optimization preference.

## 5. Expected Durable State

After both requests complete:

- `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = 'abc123'` returns `1`
- the durable row payload matches the logical order requested by both clients

The unique constraint on `idempotency_key` is the final arbiter of reality in this spec.

Redis may coordinate. Postgres must decide.

## 6. Expected Responses

Both requests must return the same logical order result.

Allowed shape:

- request A returns a created order
- request B converges to that same created order via the conflict path

Disallowed shape:

- one success and one raw conflict error
- different order IDs
- partial or structurally different responses

This is the difference between suppression and convergence.

## 7. Expected Trace

Trace must preserve both executions independently.

Minimum required evidence:

Execution A:

- `REDIS GET -> null`
- Postgres write path entered
- durable insert success or equivalent winning write

Execution B:

- `REDIS GET -> null`
- Postgres write path entered
- durable conflict observed
- existing row loaded and returned

Redis `SET` may be performed by the winning execution, skipped by the losing execution, or repeated harmlessly depending on implementation details. That is secondary.

The critical truth is:

- both executions are visible
- only one durable effect exists

## 8. Expected Replay Behavior

Replay must stay execution-scoped.

Required outcomes:

- `replay(A)` uses only execution A's recorded checkpoints
- `replay(B)` uses only execution B's recorded checkpoints
- replay does not cross-contaminate histories between A and B
- replay does not recompute a new winner

The original live contention is not replayed. The recorded outcomes are replayed.

## 9. Forbidden Outcomes

The following outcomes are illegal:

- two rows in `idempotent_orders` for `abc123`
- only one execution recorded when two requests actually ran
- Redis preventing the second request after the barrier point
- different logical responses returned to A and B
- the second request returning a raw conflict error instead of converging
- replay producing a different result than the original execution it represents
- trace collapsing the contention into a single visible attempt

## 10. Invariant Check

This spec only passes if all four layers stay aligned:

- implementation: both requests were forced past the Redis miss before the durable write
- behavior: exactly one durable row exists and both responses converge
- explanation: [checkpoint-contract.md](checkpoint-contract.md), [failure-spec-durable-write-before-checkpoint.md](failure-spec-durable-write-before-checkpoint.md), and [../examples/idempotency/README.md](../examples/idempotency/README.md) still describe what happened honestly
- evidence: trace and replay preserve both attempts without multiplying the durable outcome

In shorthand:

```text
implementation = behavior = explanation = evidence
```

## What This Spec Proves

If Flux passes this spec, it proves a stronger claim than "idempotency works under retries."

It proves:

- correctness does not depend on coordination timing
- Redis is not the correctness authority
- Postgres durable truth collapses contention safely
- application logic converges both request outcomes to the same logical result
- observability remains honest when contention duplicates history

That is the minimum bar for claiming correctness under race.