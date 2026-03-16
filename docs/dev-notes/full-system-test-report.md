# Full System Test Report

## Purpose

This report defines the practical test surface for Flux in its current runtime-first architecture.

The product promise is:

1. a developer runs backend logic inside one system
2. the system records execution, state changes, and async follow-up work
3. the developer can explain, replay, diff, and audit what happened

High-value tests should prove the execution record remains coherent across CLI, server, runtime, queue, API, and database dispatch.

## Current Architecture Scope

This report targets the current codebase shape:

- Core crates: `api`, `runtime`, `queue`, `cli`, `server`, `shared/job_contract`
- Runtime request path: single-binary `server` + in-process dispatch
- Data path: runtime database dispatch and mutation recording
- Async path: queue poller/worker lifecycle tied to request lineage

Out-of-scope historical surfaces:

- removed standalone ingress crate
- removed standalone data-service crate

## Test Philosophy

Flux is a systems product. Passing isolated unit tests is necessary but not sufficient.

Coverage should exist at six levels:

1. pure logic correctness
2. contract and serialization stability
3. persistence and transaction integrity
4. subsystem boundary behavior
5. full product-loop behavior
6. resilience under concurrency and recovery

## Scenario Taxonomy

Every critical path should map to one or more scenario families.

| Code | Scenario family | What it proves |
|---|---|---|
| `S1` | Happy path | Valid input returns expected output and state changes |
| `S2` | Input validation | Invalid body/path/query/env fails correctly |
| `S3` | Auth and authz | Missing/invalid/wrong-scope credentials are rejected |
| `S4` | Not found and conflict | Missing records, duplicate inserts, stale resources |
| `S5` | Persistence integrity | Writes, rollbacks, mutation ordering, idempotency |
| `S6` | Contract stability | JSON shape, headers, status mapping, backward compatibility |
| `S7` | Downstream failure | DB/runtime/queue/API dependency failures map predictably |
| `S8` | Retry and timeout | Backoff, retries, dead-letter, timeout recovery |
| `S9` | Concurrency and race | Parallel requests, worker contention, dedupe behavior |
| `S10` | Observability lineage | request id propagation across spans/logs/mutations/jobs |
| `S11` | Config and startup | Defaults, env precedence, startup/shutdown safety |
| `S12` | Security abuse | Token confusion, header spoofing, data leakage checks |
| `S13` | Replay and audit | History, blame, replay, diff trustworthiness |
| `S14` | Performance and soak | Latency, backlog recovery, sustained load behavior |
| `S15` | Migration and upgrade | Fresh bootstrap + upgrade safety |
| `S16` | UX contract | CLI output/exit behavior and docs-level contract |

## Ownership Matrix

| Subsystem | Direct coverage | Cross-system coverage |
|---|---|---|
| CLI | crate tests + command tests | `trace`, `why`, `records`, `invoke`, `state` flows |
| Server | route/mount tests | `/health`, `/flux/api`, `/flux/dev/invoke`, runtime ingress path |
| Runtime | executor/pool tests | execution path, span/log emission, dispatch integration |
| Queue | worker/poller/retry tests | enqueue -> run -> retry/dead-letter -> trace linkage |
| API | middleware/route tests | health, records, trace/debug/query surfaces |
| DB dispatch | integration tests on runtime/server path | mutation logging, row history, replay window |
| End-to-end loop | shell + integration tests | invoke -> request id -> trace -> why -> history |

## Product-Critical Release Gates

### 1) Core Developer Loop

The following must pass as one journey:

1. `flux init`
2. `flux dev`
3. `flux function create`
4. `flux deploy` or local invoke
5. `flux invoke`
6. `flux trace`
7. `flux why`
8. `flux records count/export`

Required checks:

- correct project/config generation
- local DB bootstrap succeeds
- monolith starts and serves expected routes
- function invocation returns stable request id
- trace and why surfaces are populated and coherent

### 2) Execution Record Integrity

Every externally visible invocation should:

- return `x-request-id`
- appear in trace listing
- have non-empty trace detail
- include correlated logs
- include correlated record export rows

### 3) State Audit Integrity

Every mutation should:

- create mutation records linked by request id
- retain correct before/after state semantics
- increment row history predictably
- remain discoverable in replay/diff windows
- preserve deterministic ordering with sequence fields

### 4) Queue and Background Lineage

Required end-to-end coverage:

- enqueue work from runtime path
- worker picks up and executes job
- retries use exponential backoff and max-attempt policy
- dead-letter behavior is inspectable
- child/background work links back to parent request lineage

### 5) Security and Isolation

Required checks:

- unauthenticated requests fail where expected
- internal endpoints enforce service token
- project scoping is honored
- replay flags do not cause unintended side effects
- records/logs do not leak raw secrets or credentials

### 6) Resilience and Recovery

Required suites:

- restart with retained DB state
- migration forward on non-empty database
- queue backlog recovery after interruption
- trace/list/read surfaces under concurrent load
- retention/pruning behavior on realistic row counts

## Contract Test Priorities

Highest-priority contracts to lock:

- CLI invoke request/response envelope
- runtime execution input/output schema
- trace detail schema (spans + timing + errors)
- mutation/history/replay response schemas
- queue job lifecycle schema and status transitions

Contract failures are release blockers because they break debugging trust.

## Suggested Command Set

Use targeted commands first, then broader gates:

```bash
cargo test -p runtime
cargo test -p queue
cargo test -p api
cargo test -p cli
cargo test -p server

make test-async-wiring
make test-product-loop
make test-platform
```

If a regression appears, validate product-loop commands directly:

```bash
flux invoke <function>
flux trace <request_id>
flux why <request_id>
```

## Beta Readiness Criteria

Flux is beta-ready when all of the following remain true under repeated runs:

- request execution is reliable
- execution record is complete and queryable
- state changes are auditable and replayable
- async work preserves lineage and observability
- failure explanations stay actionable in CLI surfaces

If a new feature cannot be validated against those criteria, it is not yet integrated into the Flux product contract.
