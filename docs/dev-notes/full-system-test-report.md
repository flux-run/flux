# Full System Test Report

> Historical note (2026-03-16): this report captures a pre-simplification architecture snapshot and contains references to deleted `gateway` and `data-engine` crates. Treat service-specific sections as archival context, not current source-of-truth.

## Purpose

This report defines the practical test surface for Flux as it exists in the repo today and as it is intended to ship as a complete product.

The product promise is not "functions run." The product promise is:

1. a developer builds and invokes backend logic inside one system
2. the system records the execution, state changes, and background work
3. the developer can explain, replay, diff, and audit what happened

That means the highest-value tests are not generic route pings. They are tests that prove the complete execution record survives across the CLI, server, gateway, runtime, queue, agents, API, and data-engine.

No finite test suite can create a literal 0% production breakage guarantee. What this report does instead is define the strongest practical beta gate from the actual source tree.

## Audit Method

This report was built from the source tree, not from the README or product copy.

- Workspace crates audited: `api`, `runtime`, `cli`, `gateway`, `queue`, `data-engine`, `server`, `agent`, `shared/job_contract`
- Additional repo surfaces included in scope: `dashboard`, `frontend`, `packages`, `projects`, `scaffolds`, `schemas`, `examples`, `scripts`
- Workspace source inventory size:
  - `api`: 60 Rust files, 286 functions
  - `runtime`: 31 Rust files, 144 functions
  - `cli`: 50 Rust files, 328 functions
  - `gateway`: 22 Rust files, 40 functions
  - `queue`: 31 Rust files, 41 functions
  - `data-engine`: 64 Rust files, 216 functions
  - `server`: 5 Rust files, 7 functions
  - `agent`: 8 Rust files, 31 functions
  - `shared/job_contract`: 3 Rust files, 25 functions
- Additional repo file counts:
  - `dashboard`: 81 files
  - `frontend`: 72 files
  - `packages`: 9 files
  - `projects`: 15 files
  - `scaffolds`: 59 files
  - `schemas`: 66 files
  - `examples`: 62 files
  - `scripts`: 28 files

## What "Complete Coverage" Means For Flux

Flux is a systems product. "Unit tests pass" is not enough.

Coverage has to exist at six levels:

1. Pure logic
2. Serialization and contract stability
3. Persistence and transaction behavior
4. Service-to-service boundary behavior
5. Full product-loop behavior
6. Long-run resilience under concurrency, retries, and restarts

## Scenario Taxonomy

Every function in the audited source tree should be covered by one or more of these scenario families.

| Code | Scenario family | What it proves |
|---|---|---|
| `S1` | Happy path | Valid input returns the expected output and state changes |
| `S2` | Input validation | Invalid body, path, query, or env input fails correctly |
| `S3` | Auth and authz | Missing, malformed, expired, or wrong-scope credentials are rejected |
| `S4` | Not found and conflict | Missing records, duplicate inserts, version conflicts, stale resources |
| `S5` | Persistence integrity | DB writes, transactions, rollbacks, mutation ordering, idempotency |
| `S6` | Contract stability | JSON shape, headers, status codes, type serialization, backward compatibility |
| `S7` | Downstream failure | DB outage, runtime failure, API failure, object storage failure, LLM failure |
| `S8` | Retry and timeout | Backoff, retries, DLQ, timeout recovery, cancellation, idempotent re-run |
| `S9` | Concurrency and race | Parallel requests, snapshot swaps, cache invalidation, duplicate workers |
| `S10` | Observability lineage | `x-request-id`, trace propagation, logs, records, mutation linkage |
| `S11` | Config and startup | Defaults, env precedence, invalid env, startup failure, shutdown safety |
| `S12` | Security abuse | Header spoofing, payload size abuse, token confusion, secret leakage |
| `S13` | Replay and audit | history, blame, replay, diff, deterministic explanation |
| `S14` | Performance and soak | latency, backlog, retention jobs, export size, sustained load |
| `S15` | Migration and upgrade | fresh DB bootstrap, upgrade from old schema, backward-compatible changes |
| `S16` | UX contract | CLI exit codes, human output, JSON mode, docs/examples/scaffolds staying valid |

## File-Owner Rules

The source tree is large enough that exhaustive testing must be organized by ownership pattern. Every function inherits the scenario families for its file type.

| File owner pattern | Required scenario families |
|---|---|
| `main.rs`, bootstraps, binaries | `S1`, `S7`, `S11`, `S14`, `S15` |
| `config/*`, settings loaders | `S2`, `S11`, `S15` |
| `routes/*`, `handlers/*`, command executors | `S1`, `S2`, `S3`, `S4`, `S6`, `S7`, `S10`, `S12`, `S16` |
| `middleware/*` | `S1`, `S3`, `S6`, `S10`, `S12` |
| `services/*`, `dispatch/*` | `S1`, `S5`, `S6`, `S7`, `S8`, `S10` |
| `db/*`, stateful persistence helpers | `S1`, `S5`, `S7`, `S9`, `S15` |
| `worker/*`, schedulers, pollers | `S1`, `S5`, `S7`, `S8`, `S9`, `S10`, `S14` |
| `engine/*`, `compiler/*`, `policy/*`, `schema/*`, `transform/*`, caches | `S1`, `S2`, `S5`, `S6`, `S9`, `S13`, `S14` |
| `models/*`, `types/*`, DTOs | `S2`, `S6`, `S15` |
| examples, scaffolds, packages | `S1`, `S6`, `S16` |
| migrations and schema SQL | `S1`, `S5`, `S15` |
| shell scripts | `S1`, `S7`, `S11`, `S16` |

## Product-Critical Release Gates

These are the gates that matter most for Flux's product goal.

### 1. Core developer loop

The following must pass as a single end-to-end journey:

1. `flux init`
2. `flux dev`
3. `flux function create`
4. `flux deploy` or local invoke
5. `flux invoke`
6. `flux trace`
7. `flux why`
8. `flux records count/export`

Required checks:

- correct config generation
- local DB bootstrap
- monolith starts on one port
- function executes through gateway or dev invoke path
- returned `x-request-id` can be used by CLI follow-up commands
- trace, logs, records, and mutations agree on the same request

### 2. Execution record loop

Every externally visible execution must produce:

- request id
- trace row
- span/log rows
- records export visibility
- if state changed, mutation log entries
- if background work fired, child lineage

### 3. State audit loop

Every data mutation must support:

- mutation list by request id
- row history lookup
- blame lookup
- replay window lookup
- stable `before_state` and `after_state`
- stable ordering by `mutation_seq`

### 4. Background lineage loop

Queue, schedules, events, and agents cannot be orphan work.

The system must prove:

- the origin request can be traced to queued work
- retries and DLQ activity remain inspectable
- cron and event workers create visible execution history
- agent tool calls and LLM steps stay attributable

### 5. Monolith parity loop

Standalone services and single-binary mode must match on:

- auth behavior
- route availability
- request id behavior
- record and mutation creation
- CLI compatibility

## Subsystem Test Matrix

### CLI

Primary modules:

- connectivity and config: `context.rs`, `config.rs`, `client.rs`, `auth.rs`, `api_key.rs`, `env_cmd.rs`
- project bootstrap and scaffolding: `init.rs`, `create.rs`, `new_function.rs`, `functions.rs`, `toolchain.rs`, `generate.rs`
- local runtime orchestration: `dev.rs`, `server.rs`, `stack.rs`
- deployment lifecycle: `deploy.rs`, `deployments.rs`, `version_cmd.rs`
- debug surface: `invoke.rs`, `trace.rs`, `trace_diff.rs`, `trace_debug.rs`, `why.rs`, `doctor.rs`, `incident.rs`, `bisect.rs`, `debug.rs`, `errors.rs`
- data and platform management: `db.rs`, `db_push.rs`, `gateway.rs`, `queue.rs`, `agent.rs`, `schedule.rs`, `event.rs`, `records.rs`, `logs.rs`, `monitor.rs`, `sdk.rs`, `open.rs`, `upgrade.rs`, `config_cmd.rs`, `state.rs`

Required tests:

- config precedence across env, file, context, and defaults
- every command supports both human output and `--json` where advertised
- every command exits non-zero on actionable errors
- `flux dev` bootstraps DB, starts server, and waits for health
- `flux invoke` works in both gateway and dev-invoke mode
- `trace`, `why`, `trace diff`, `doctor`, `records` never panic on missing or malformed data
- `deploy` handles JS and WASM packaging, upload failures, and hash dedupe
- scaffolded projects compile or at least validate for every supported runtime template
- backward-compatible aliases do not drift from actual command behavior

Priority scenarios:

- `S1`, `S2`, `S6`, `S10`, `S11`, `S16`
- plus `S7` on all networked commands

### Server

Primary modules:

- `main.rs`
- `dispatch/api_impl.rs`
- `dispatch/runtime_impl.rs`
- `dispatch/agent_impl.rs`

Required tests:

- one-port mount table: `/flux/api`, `/flux`, wildcard gateway, `/flux/dev/invoke/{name}`
- in-process dispatch parity with HTTP-backed services
- startup failure when DB is absent or invalid
- local mode seeding
- dashboard/static mount fallback behavior
- TLS branch and plain HTTP branch
- monolith request path still writes the same records and traces as split mode

Priority scenarios:

- `S1`, `S7`, `S10`, `S11`, `S15`

### Gateway

Primary modules:

- router and handlers: `router.rs`, `handlers/dispatch.rs`, `handlers/health.rs`, `handlers/readiness.rs`
- auth: `auth/mod.rs`, `auth/api_key.rs`, `auth/jwt.rs`
- rate limiting and observability: `rate_limit/mod.rs`, `metrics.rs`, `trace/mod.rs`
- route snapshot: `snapshot/mod.rs`, `snapshot/store.rs`, `snapshot/types.rs`
- forwarding and config: `forward/http_impl.rs`, `config.rs`
- operational binaries: `bin/migrate.rs`, `bin/seed.rs`

Required tests:

- route hit, route miss, method mismatch
- snapshot refresh and LISTEN/NOTIFY update propagation
- JWT and API-key auth matrix
- local-mode bypass versus non-local enforcement
- body size limits, content-length abuse, CORS preflight
- rate limit correctness and keying
- request id precedence and `traceparent` propagation
- readiness flips only when snapshot is loaded
- downstream runtime errors map to stable gateway responses

Priority scenarios:

- `S1`, `S3`, `S6`, `S9`, `S10`, `S12`

### Runtime

Primary modules:

- bootstrap and state: `main.rs`, `lib.rs`, `state.rs`, `config/settings.rs`
- execution path: `execute/handler.rs`, `execute/runner.rs`, `execute/service.rs`, `execute/types.rs`, `execute/bundle.rs`, `execute/invalidate.rs`
- engines: `engine/executor.rs`, `engine/pool.rs`, `engine/wasm_executor.rs`, `engine/wasm_pool.rs`
- bundle and schema caching: `bundle/cache.rs`, `schema/cache.rs`
- downstream dispatch and secrets: `dispatch/http_api.rs`, `dispatch/http_queue.rs`, `secrets/client.rs`
- trace and agent LLM: `trace/emitter.rs`, `agent/mod.rs`, `agent/llm.rs`

Required tests:

- direct `/execute` contract
- JS and WASM execution parity
- bundle resolution order: warm cache, cold fetch, inline bundle, object storage
- schema cache hits and invalidation
- secret fetch, cache, and invalidation
- queue push dispatch behavior
- runtime trace emission and log emission
- isolation pool behavior under concurrency
- allowed outbound host restrictions for WASM HTTP
- execution error mapping, panics, timeouts, malformed bundle responses

Priority scenarios:

- `S1`, `S5`, `S7`, `S8`, `S9`, `S10`, `S12`, `S13`

### Queue

Primary modules:

- bootstrap and state: `main.rs`, `state.rs`, `config/config.rs`, `db/connection.rs`
- public API: `api/routes.rs`, `api/handlers/create_job.rs`, `get_job.rs`, `list_jobs.rs`, `cancel_job.rs`, `retry_job.rs`, `stats.rs`
- core services: `services/job_service.rs`, `services/retry_service.rs`
- queue storage: `queue/fetch_jobs.rs`, `queue/update_status.rs`
- worker execution: `worker/worker.rs`, `worker/poller.rs`, `worker/executor.rs`, `worker/backoff.rs`, `worker/timeout_recovery.rs`, `worker/span_emitter.rs`
- upstream bridge: `dispatch.rs`

Required tests:

- create, list, get, cancel, retry, stats
- lock and fetch semantics under concurrency
- retry schedule and dead-letter transitions
- worker success, worker failure, worker timeout
- timeout recovery moves stuck jobs correctly
- idempotent status transitions
- queue span emission and request-id propagation
- runtime/API downstream failure handling

Priority scenarios:

- `S1`, `S5`, `S7`, `S8`, `S9`, `S10`, `S14`

### Agents

Primary modules:

- public runtime surface: `lib.rs`
- definitions and validation: `schema.rs`, `registry.rs`
- execution and policy: `loop_runner.rs`, `rules.rs`, `tools.rs`
- LLM and audit: `llm.rs`, `recording.rs`

Required tests:

- YAML parse and validation
- deploy/list/get/delete lifecycle
- agent run with deterministic mocked LLM
- tool schema generation and DB-backed tool discovery
- rules state transitions
- per-step recording to DB
- LLM request/response failure handling
- request lineage between agent run, tool call, and runtime execution

Priority scenarios:

- `S1`, `S2`, `S5`, `S7`, `S10`, `S13`

### API

Primary modules:

- bootstrap and state: `main.rs`, `lib.rs`, `app.rs`, `config/mod.rs`
- middleware: `middleware/auth.rs`, `middleware/internal_auth.rs`, `middleware/request_id.rs`
- auth: `auth/routes.rs`, `auth/service.rs`, `auth/models.rs`
- errors and shared types: `error.rs`, `types/context.rs`, `types/response.rs`
- services: `services/storage.rs`, `services/slug_service.rs`
- secrets: `secrets/*`
- logs and traces: `logs/routes.rs`
- DB and models: `db/*`, `models/*`
- routes:
  - functions, deployments, manifest
  - gateway config and gateway routes
  - schema, sdk, openapi, spec, introspect
  - db migrate, data_engine proxy
  - api keys, records, monitor
  - events, queue management, schedules
  - agents, environments, stream
  - stubs and system

Required tests:

- every public route path in `app.rs`
- every internal route path in `app.rs`
- auth matrix: JWT, API key, local mode, missing headers, project override header
- request id injection
- internal service token enforcement
- storage service behavior with local mode and S3-compatible backends
- deploy and activation flows including rollback
- record export/count/prune correctness
- log and trace lookup correctness
- proxy behavior to data-engine with forwarded auth/service tokens
- events, queue, schedules, agents, environments CRUD contract
- stream endpoints for event, execution, and mutation feeds

Priority scenarios:

- `S1`, `S3`, `S5`, `S6`, `S7`, `S10`, `S12`, `S13`, `S15`

### Data-engine

Primary modules:

- bootstrap, config, state: `main.rs`, `config.rs`, `state.rs`, `telemetry.rs`
- public API: `api/routes.rs`, `api/middleware/service_auth.rs`, `api/handlers/*`
- database routing and connections: `router/db_router.rs`, `db/connection.rs`
- query path: `compiler/query_compiler.rs`, `compiler/relational.rs`, `query_guard.rs`, `engine/pipeline.rs`, `executor/db_executor.rs`, `executor/batched.rs`
- schema and policy: `schema/eval.rs`, `schema/rules.rs`, `schema/hooks.rs`, `schema/events.rs`, `engine/schema_rules.rs`, `policy/engine.rs`
- hooks, transforms, files: `hooks/engine.rs`, `transform/engine.rs`, `file_engine/engine.rs`
- background workers: `events/emitter.rs`, `events/dispatcher.rs`, `events/worker.rs`, `cron/worker.rs`, `retention/worker.rs`
- cache and invalidation: `cache/mod.rs`, `cache/manager.rs`, `cache/invalidation.rs`

Required tests:

- every `/db/*` and `/files/*` route
- service token enforcement, health/version exemptions, header normalization
- query compile and execute for select/insert/update/delete
- depth and complexity limits
- schema introspection and schema-driven rule evaluation
- policy application and auth context routing
- hook execution on create/update/delete
- relationships and subscriptions lifecycle
- cron create/update/delete/trigger and worker delivery
- events emission, matching, delivery retries, and dedupe
- mutation history, blame, replay, and export shape
- file upload/download URL generation and object-key integrity
- retention pruning on realistic row counts
- cache invalidation across LISTEN/NOTIFY

Priority scenarios:

- `S1`, `S5`, `S7`, `S8`, `S9`, `S10`, `S13`, `S14`, `S15`

### Shared job contract

Primary modules:

- `dispatch.rs`
- `job.rs`
- `lib.rs`

Required tests:

- request and response serialization round-trips
- optional field defaults
- object-safe trait usage
- backward-compatible JSON contract for queue/runtime/API boundaries

Priority scenarios:

- `S6`, `S15`

## Integration Boundary Matrix

This is where Flux either becomes coherent or falls apart.

| Boundary | Required checks |
|---|---|
| CLI -> Server/API | auth header formation, base URL resolution, exit-code stability, JSON contract stability |
| CLI -> Gateway | invocation path, request-id extraction, payload encoding |
| Server -> API | in-process parity with standalone API router |
| Server -> Runtime | in-process execute parity, dev invoke parity |
| Server -> Agent | agent dispatch and recording parity |
| Gateway -> Runtime | execute request shape, downstream timeout and status mapping |
| Gateway -> DB snapshot | snapshot freshness, readiness semantics, route invalidation |
| Runtime -> API | bundle fetch, secret fetch, log write contract |
| Runtime -> Queue | push job contract and request lineage |
| API -> Data-engine | proxied auth, service token, path forwarding, method forwarding |
| API -> Storage | local mode versus S3-compatible object storage |
| Queue -> Runtime | job execution request shape, retry behavior |
| Queue -> API | span emission and secret/bundle fetch contract |
| Agent -> Runtime | tool call execution and lineage |
| Agent -> LLM | stable payload shape, model config, error handling |
| Data-engine -> Runtime | hooks, cron, and events dispatch contract |
| Data-engine -> Postgres | transactionality, search path, locking, mutation logging |
| Data-engine -> Object storage | upload/download key stability and URL generation |
| Gateway/Data-engine caches -> LISTEN/NOTIFY | invalidation under concurrent updates |

## Non-Workspace Repo Surfaces

These are part of the repo and need explicit tests even though they are not core Rust crates.

### Dashboard

Required coverage:

- route rendering for traces, functions, data, cron, agents, routes, logs, secrets, monitor
- API client header correctness, especially project header and auth behavior
- auth/login flow
- regression tests for pages that depend on `/traces`, `/functions`, `/db/*`, `/agents`, `/schedules`
- build and static export smoke

### Frontend

Required coverage:

- production build
- docs links and navigation integrity
- pricing calculator behavior
- docs sidebar route validity
- site copy links to product docs and product commands

### Packages

Required coverage:

- type exports stay backward-compatible
- runtime helper behavior matches generated schema expectations
- package build and publish smoke

### Projects, examples, scaffolds

Required coverage:

- every example project boots or validates
- every function scaffold produces a valid `flux.json`
- template snapshots remain stable
- language-specific hello-world examples compile or at least validate with the declared toolchain
- scaffolded project docs remain aligned with CLI output

### Schemas

Required coverage:

- migrate from empty DB to head
- migrate from previous known good baseline to head
- idempotent startup
- expected tables, columns, triggers, indexes, and functions exist after migration
- queue schema and API schema stay compatible with code assumptions

### Scripts

Required coverage:

- shell syntax validation
- environment-variable validation
- happy-path smoke in CI
- failure messages that tell the operator what is missing

## Current Coverage Gaps From The Source Audit

This is the current direct-test posture by crate, based on file-level test markers in source files.

| Crate | Files without direct tests | Total source files | Main risk |
|---|---:|---:|---|
| `cli` | 49 | 50 | product DX can drift without compile-time alarms |
| `server` | 5 | 5 | monolith parity can break silently |
| `api` | 55 | 60 | route and auth regressions likely unless covered by system tests |
| `runtime` | 26 | 31 | boundary caches and trace emission still under-protected |
| `gateway` | 20 | 22 | auth/routing/snapshot edges are under-tested |
| `queue` | 30 | 31 | worker and retry correctness mostly depend on runtime behavior |
| `data-engine` | 58 | 64 | large surface with compiler, policy, hooks, events, cron, retention |
| `agent` | 7 | 8 | LLM, recording, and runtime-tool lineage mostly untested |
| `shared/job_contract` | 1 | 3 | comparatively healthy, but still needs compatibility discipline |

The current source tree already has some strong unit tests in:

- runtime engine and bundle code
- data-engine `query_guard`, `schema/rules`, `schema/hooks`, `history`, and service-auth middleware
- API `error`, `auth/service`, `secrets/encryption`, and internal auth middleware
- gateway trace parsing and router basics
- queue route health/version checks
- agent schema validation
- shared job contract serialization

The weakest areas remain:

- CLI command behavior versus real service contracts
- monolithic server behavior
- API route families beyond middleware and helpers
- queue worker behavior under retries and timeouts
- agent loop execution and recording
- data-engine background workers and file/storage features

## Required Test Suites Before 0.1 Beta

### Unit and property suites

- compiler, query guard, schema rules, hook transforms
- bundle hashing and cache behavior
- CLI diff, explain, trace rendering, and config precedence
- DTO and contract round-trips
- rate-limit and trace header parsing

### Contract suites

- API route JSON snapshots for stable endpoints
- job contract JSON snapshots
- gateway-to-runtime and runtime-to-API payload snapshots
- agent-to-LLM request snapshots

### Integration suites

- split-services mode with real Postgres
- monolith mode with real Postgres
- S3-compatible object storage mode
- agent run with mocked LLM and real runtime
- queue workers with real retry timing

### Product-loop suites

- `init -> dev -> invoke -> trace -> why`
- request execution -> record export -> mutation history -> replay
- queue publish -> worker execute -> trace lineage
- schedule create -> run -> history -> trace lineage
- event publish -> subscription dispatch -> trace lineage
- agent run -> tool call -> recorded steps -> trace linkage

### Soak and chaos suites

- restart during active queue backlog
- restart during cache invalidation traffic
- retention worker under large record volume
- trace and records endpoints under concurrent reads
- DB failover or network interruption simulation for workers

## Audited Workspace Source Inventory

The following backend source files were explicitly included in the audit and inherit the test-owner rules above.

### `api`

- `api/src/app.rs`
- `api/src/auth/mod.rs`
- `api/src/auth/models.rs`
- `api/src/auth/routes.rs`
- `api/src/auth/service.rs`
- `api/src/config/mod.rs`
- `api/src/db/connection.rs`
- `api/src/db/mod.rs`
- `api/src/db/queries.rs`
- `api/src/error.rs`
- `api/src/lib.rs`
- `api/src/logs/mod.rs`
- `api/src/logs/routes.rs`
- `api/src/main.rs`
- `api/src/middleware/auth.rs`
- `api/src/middleware/internal_auth.rs`
- `api/src/middleware/mod.rs`
- `api/src/middleware/request_id.rs`
- `api/src/models/membership.rs`
- `api/src/models/mod.rs`
- `api/src/models/project.rs`
- `api/src/models/tenant.rs`
- `api/src/models/user.rs`
- `api/src/routes/agents.rs`
- `api/src/routes/api_keys.rs`
- `api/src/routes/data_engine.rs`
- `api/src/routes/db_migrate.rs`
- `api/src/routes/deployments.rs`
- `api/src/routes/environments.rs`
- `api/src/routes/events.rs`
- `api/src/routes/functions.rs`
- `api/src/routes/gateway_config.rs`
- `api/src/routes/gateway_routes.rs`
- `api/src/routes/introspect.rs`
- `api/src/routes/manifest.rs`
- `api/src/routes/mod.rs`
- `api/src/routes/monitor.rs`
- `api/src/routes/openapi.rs`
- `api/src/routes/queue_mgmt.rs`
- `api/src/routes/records.rs`
- `api/src/routes/schedules.rs`
- `api/src/routes/schema.rs`
- `api/src/routes/sdk.rs`
- `api/src/routes/spec.rs`
- `api/src/routes/stream.rs`
- `api/src/routes/stubs.rs`
- `api/src/routes/system.rs`
- `api/src/secrets/dto.rs`
- `api/src/secrets/encryption.rs`
- `api/src/secrets/events.rs`
- `api/src/secrets/mod.rs`
- `api/src/secrets/model.rs`
- `api/src/secrets/routes.rs`
- `api/src/secrets/service.rs`
- `api/src/services/mod.rs`
- `api/src/services/slug_service.rs`
- `api/src/services/storage.rs`
- `api/src/types/context.rs`
- `api/src/types/mod.rs`
- `api/src/types/response.rs`

### `runtime`

- `runtime/src/agent/llm.rs`
- `runtime/src/agent/mod.rs`
- `runtime/src/bundle/cache.rs`
- `runtime/src/bundle/mod.rs`
- `runtime/src/config/mod.rs`
- `runtime/src/config/settings.rs`
- `runtime/src/dispatch/http_api.rs`
- `runtime/src/dispatch/http_queue.rs`
- `runtime/src/dispatch/mod.rs`
- `runtime/src/engine/executor.rs`
- `runtime/src/engine/mod.rs`
- `runtime/src/engine/pool.rs`
- `runtime/src/engine/wasm_executor.rs`
- `runtime/src/engine/wasm_pool.rs`
- `runtime/src/execute/bundle.rs`
- `runtime/src/execute/handler.rs`
- `runtime/src/execute/invalidate.rs`
- `runtime/src/execute/mod.rs`
- `runtime/src/execute/runner.rs`
- `runtime/src/execute/service.rs`
- `runtime/src/execute/types.rs`
- `runtime/src/lib.rs`
- `runtime/src/main.rs`
- `runtime/src/schema/cache.rs`
- `runtime/src/schema/mod.rs`
- `runtime/src/schema/validator.rs`
- `runtime/src/secrets/client.rs`
- `runtime/src/secrets/mod.rs`
- `runtime/src/state.rs`
- `runtime/src/trace/emitter.rs`
- `runtime/src/trace/mod.rs`

### `cli`

- `cli/src/agent.rs`
- `cli/src/api_key.rs`
- `cli/src/auth.rs`
- `cli/src/bisect.rs`
- `cli/src/client.rs`
- `cli/src/config.rs`
- `cli/src/config_cmd.rs`
- `cli/src/context.rs`
- `cli/src/create.rs`
- `cli/src/db.rs`
- `cli/src/db_push.rs`
- `cli/src/debug.rs`
- `cli/src/deploy.rs`
- `cli/src/deployments.rs`
- `cli/src/dev.rs`
- `cli/src/doctor.rs`
- `cli/src/env_cmd.rs`
- `cli/src/errors.rs`
- `cli/src/event.rs`
- `cli/src/explain.rs`
- `cli/src/functions.rs`
- `cli/src/gateway.rs`
- `cli/src/generate.rs`
- `cli/src/incident.rs`
- `cli/src/init.rs`
- `cli/src/invoke.rs`
- `cli/src/logs.rs`
- `cli/src/main.rs`
- `cli/src/monitor.rs`
- `cli/src/new_function.rs`
- `cli/src/open.rs`
- `cli/src/projects.rs`
- `cli/src/queue.rs`
- `cli/src/records.rs`
- `cli/src/schedule.rs`
- `cli/src/sdk.rs`
- `cli/src/secrets.rs`
- `cli/src/server.rs`
- `cli/src/stack.rs`
- `cli/src/state.rs`
- `cli/src/tail.rs`
- `cli/src/tenant.rs`
- `cli/src/toolchain.rs`
- `cli/src/trace.rs`
- `cli/src/trace_debug.rs`
- `cli/src/trace_diff.rs`
- `cli/src/upgrade.rs`
- `cli/src/version_cmd.rs`
- `cli/src/whoami.rs`
- `cli/src/why.rs`

### `gateway`

- `gateway/src/auth/api_key.rs`
- `gateway/src/auth/jwt.rs`
- `gateway/src/auth/mod.rs`
- `gateway/src/bin/migrate.rs`
- `gateway/src/bin/seed.rs`
- `gateway/src/config.rs`
- `gateway/src/forward/http_impl.rs`
- `gateway/src/forward/mod.rs`
- `gateway/src/handlers/dispatch.rs`
- `gateway/src/handlers/health.rs`
- `gateway/src/handlers/mod.rs`
- `gateway/src/handlers/readiness.rs`
- `gateway/src/lib.rs`
- `gateway/src/main.rs`
- `gateway/src/metrics.rs`
- `gateway/src/rate_limit/mod.rs`
- `gateway/src/router.rs`
- `gateway/src/snapshot/mod.rs`
- `gateway/src/snapshot/store.rs`
- `gateway/src/snapshot/types.rs`
- `gateway/src/state.rs`
- `gateway/src/trace/mod.rs`

### `queue`

- `queue/src/api/handlers/cancel_job.rs`
- `queue/src/api/handlers/create_job.rs`
- `queue/src/api/handlers/get_job.rs`
- `queue/src/api/handlers/list_jobs.rs`
- `queue/src/api/handlers/retry_job.rs`
- `queue/src/api/handlers/stats.rs`
- `queue/src/api/mod.rs`
- `queue/src/api/routes.rs`
- `queue/src/config/config.rs`
- `queue/src/config/mod.rs`
- `queue/src/db/connection.rs`
- `queue/src/db/mod.rs`
- `queue/src/dispatch.rs`
- `queue/src/lib.rs`
- `queue/src/main.rs`
- `queue/src/models/job.rs`
- `queue/src/models/mod.rs`
- `queue/src/queue/fetch_jobs.rs`
- `queue/src/queue/mod.rs`
- `queue/src/queue/update_status.rs`
- `queue/src/services/job_service.rs`
- `queue/src/services/mod.rs`
- `queue/src/services/retry_service.rs`
- `queue/src/state.rs`
- `queue/src/worker/backoff.rs`
- `queue/src/worker/executor.rs`
- `queue/src/worker/mod.rs`
- `queue/src/worker/poller.rs`
- `queue/src/worker/span_emitter.rs`
- `queue/src/worker/timeout_recovery.rs`
- `queue/src/worker/worker.rs`

### `data-engine`

- `data-engine/src/api/handlers/cron.rs`
- `data-engine/src/api/handlers/databases.rs`
- `data-engine/src/api/handlers/debug.rs`
- `data-engine/src/api/handlers/explain.rs`
- `data-engine/src/api/handlers/files.rs`
- `data-engine/src/api/handlers/history.rs`
- `data-engine/src/api/handlers/hooks.rs`
- `data-engine/src/api/handlers/mod.rs`
- `data-engine/src/api/handlers/mutations.rs`
- `data-engine/src/api/handlers/policies.rs`
- `data-engine/src/api/handlers/query.rs`
- `data-engine/src/api/handlers/relationships.rs`
- `data-engine/src/api/handlers/schema.rs`
- `data-engine/src/api/handlers/subscriptions.rs`
- `data-engine/src/api/handlers/tables.rs`
- `data-engine/src/api/middleware/mod.rs`
- `data-engine/src/api/middleware/service_auth.rs`
- `data-engine/src/api/mod.rs`
- `data-engine/src/api/routes.rs`
- `data-engine/src/cache/invalidation.rs`
- `data-engine/src/cache/manager.rs`
- `data-engine/src/cache/mod.rs`
- `data-engine/src/compiler/mod.rs`
- `data-engine/src/compiler/query_compiler.rs`
- `data-engine/src/compiler/relational.rs`
- `data-engine/src/config.rs`
- `data-engine/src/cron/mod.rs`
- `data-engine/src/cron/worker.rs`
- `data-engine/src/db/connection.rs`
- `data-engine/src/db/mod.rs`
- `data-engine/src/engine/auth_context.rs`
- `data-engine/src/engine/error.rs`
- `data-engine/src/engine/mod.rs`
- `data-engine/src/engine/pipeline.rs`
- `data-engine/src/engine/schema_rules.rs`
- `data-engine/src/events/dispatcher.rs`
- `data-engine/src/events/emitter.rs`
- `data-engine/src/events/mod.rs`
- `data-engine/src/events/worker.rs`
- `data-engine/src/executor/batched.rs`
- `data-engine/src/executor/db_executor.rs`
- `data-engine/src/executor/mod.rs`
- `data-engine/src/file_engine/engine.rs`
- `data-engine/src/file_engine/mod.rs`
- `data-engine/src/hooks/engine.rs`
- `data-engine/src/hooks/mod.rs`
- `data-engine/src/lib.rs`
- `data-engine/src/main.rs`
- `data-engine/src/policy/engine.rs`
- `data-engine/src/policy/mod.rs`
- `data-engine/src/query_guard.rs`
- `data-engine/src/retention/mod.rs`
- `data-engine/src/retention/worker.rs`
- `data-engine/src/router/db_router.rs`
- `data-engine/src/router/mod.rs`
- `data-engine/src/schema/eval.rs`
- `data-engine/src/schema/events.rs`
- `data-engine/src/schema/hooks.rs`
- `data-engine/src/schema/mod.rs`
- `data-engine/src/schema/rules.rs`
- `data-engine/src/state.rs`
- `data-engine/src/telemetry.rs`
- `data-engine/src/transform/engine.rs`
- `data-engine/src/transform/mod.rs`

### `server`

- `server/src/dispatch/agent_impl.rs`
- `server/src/dispatch/api_impl.rs`
- `server/src/dispatch/mod.rs`
- `server/src/dispatch/runtime_impl.rs`
- `server/src/main.rs`

### `agent`

- `agent/src/lib.rs`
- `agent/src/llm.rs`
- `agent/src/loop_runner.rs`
- `agent/src/recording.rs`
- `agent/src/registry.rs`
- `agent/src/rules.rs`
- `agent/src/schema.rs`
- `agent/src/tools.rs`

### `shared/job_contract`

- `shared/job_contract/src/dispatch.rs`
- `shared/job_contract/src/job.rs`
- `shared/job_contract/src/lib.rs`

## Public Surface Inventory

These are the user-facing and service-facing surfaces that need stable tests because they are the first places product drift shows up.

### CLI command surface

Commands extracted from `cli/src/main.rs`:

- `login`
- `admin-setup`
- `whoami`
- `tenant`
- `project`
- `function`
- `toolchain`
- `deploy`
- `invoke`
- `version`
- `deployments`
- `dev`
- `new`
- `create`
- `init`
- `explain`
- `trace diff`
- `trace debug`
- `bug`
- `why`
- `state`
- `incident`
- `logs`
- `trace`
- `debug`
- `fix`
- `errors`
- `tail`
- `monitor`
- `secrets`
- `config`
- `api-key`
- `gateway`
- `agent`
- `schedule`
- `queue`
- `event`
- `records`
- `env`
- `db`
- `db-push`
- `pull`
- `watch`
- `status`
- `generate`
- `server`
- `stack`
- `doctor`
- `open`
- `upgrade`
- `link`
- `use`
- `context`
- `unlink`
- `up`
- `down`
- `ps`
- `logs` under stack/server lifecycle
- `reset`
- `seed`

Each command should be covered for:

- command parsing
- config resolution
- human output
- JSON output when supported
- exit codes
- downstream failure messaging

### API route surface

Routes extracted from `api/src/app.rs`:

- internal:
  - `/internal/secrets`
  - `/internal/bundle`
  - `/internal/introspect`
  - `/internal/introspect/manifest`
  - `/internal/db/migrate`
  - `/internal/db/schema`
  - `/internal/logs`
  - `/internal/functions/resolve`
  - `/internal/cache/invalidate`
  - `/internal/routes`
- public management and data plane:
  - `/functions`
  - `/functions/{id}`
  - `/functions/deploy`
  - `/deployments`
  - `/deployments/list/{id}`
  - `/deployments/{id}/activate/{version}`
  - `/deployments/hashes`
  - `/deployments/project`
  - `/deployments/project/{id}/rollback`
  - `/secrets`
  - `/secrets/{key}`
  - `/logs`
  - `/traces`
  - `/traces/{request_id}`
  - `/gateway/routes`
  - `/gateway/routes/{id}`
  - `/gateway/middleware`
  - `/gateway/middleware/{route}/{type}`
  - `/gateway/routes/{id}/rate-limit`
  - `/gateway/routes/{id}/cors`
  - `/schema/graph`
  - `/sdk/schema`
  - `/sdk/typescript`
  - `/sdk/manifest`
  - `/openapi.json`
  - `/spec`
  - `/db/{*path}`
  - `/files/{*path}`
  - `/api-keys`
  - `/api-keys/{id}`
  - `/api-keys/{id}/rotate`
  - `/records/export`
  - `/records/count`
  - `/records/prune`
  - `/monitor/status`
  - `/monitor/metrics`
  - `/monitor/alerts`
  - `/monitor/alerts/{id}`
  - `/events`
  - `/events/subscriptions`
  - `/events/subscriptions/{id}`
  - `/queues`
  - `/queues/{name}`
  - `/queues/{name}/messages`
  - `/queues/{name}/bindings`
  - `/queues/{name}/purge`
  - `/queues/{name}/dlq`
  - `/queues/{name}/dlq/replay`
  - `/schedules`
  - `/schedules/{name}`
  - `/schedules/{name}/pause`
  - `/schedules/{name}/resume`
  - `/schedules/{name}/run`
  - `/schedules/{name}/history`
  - `/agents`
  - `/agents/{name}`
  - `/agents/{name}/run`
  - `/agents/{name}/simulate`
  - `/environments`
  - `/environments/clone`
  - `/environments/{name}`
  - `/routes`
  - `/routes/sync`
  - `/stream/events`
  - `/stream/executions`
  - `/stream/mutations`
  - `/auth/status`
  - `/auth/setup`
  - `/auth/login`
  - `/auth/logout`
  - `/auth/me`
  - `/auth/users`
  - `/auth/users/{id}`
  - `/openapi/ui`
  - `/health`
  - `/version`

The execution-plane block routes must also be tested:

- `/run`
- `/run/{*path}`
- `/invoke`
- `/invoke/{*path}`
- `/execute`
- `/execute/{*path}`
- `/functions/{name}/run`
- `/functions/{name}/invoke`

### Data-engine route surface

Routes extracted from `data-engine/src/api/routes.rs`:

- `/db/query`
- `/db/databases`
- `/db/databases/{name}`
- `/db/tables`
- `/db/tables/{database}`
- `/db/tables/{database}/{table}`
- `/db/policies`
- `/db/policies/{id}`
- `/db/hooks`
- `/db/hooks/{id}`
- `/db/relationships`
- `/db/relationships/{id}`
- `/db/subscriptions`
- `/db/subscriptions/{id}`
- `/db/cron`
- `/db/cron/{id}`
- `/db/cron/{id}/trigger`
- `/db/history/{database}/{table}`
- `/db/blame/{database}/{table}`
- `/db/replay/{database}`
- `/db/mutations`
- `/db/schema`
- `/db/debug`
- `/db/explain`
- `/files/upload-url`
- `/files/download-url`
- `/health`
- `/version`

### Gateway route surface

Routes extracted from `gateway/src/router.rs`:

- `/health`
- `/readiness`
- `/{*path}`

### Runtime route surface

Routes extracted from `runtime/src/main.rs`:

- `/health`
- `/version`
- `/execute`
- `/internal/cache/invalidate`

### Queue route surface

Routes extracted from `queue/src/api/routes.rs`:

- `/jobs`
- `/jobs/stats`
- `/jobs/{id}`
- `/jobs/{id}/retry`
- `/health`
- `/version`

### Monolith-only route surface

Routes extracted from `server/src/main.rs`:

- `/flux/dev/invoke/{name}`
- nested `/flux/api/*`
- nested `/flux/*`
- wildcard gateway mount at root

## Recommended Output Artifacts

This report should drive three concrete artifacts:

1. crate-local unit and contract test files
2. end-to-end system suites in `scripts/platform-tests`
3. CI gates that run:
   - fast unit suites on every PR
   - split-service integration suites on main
   - monolith parity suites before beta cuts

If a new feature is added and it does not fit into this matrix, it is not yet integrated into the Flux product story.
