# Testing Flux

Flux is not just a functions runtime. The product promise is:

1. a request executes through one system
2. the execution produces a durable record
3. developers can explain what happened from that record

This test plan is built around that promise.

Detailed subsystem and file-level audit:

- [full-system-test-report.md](./full-system-test-report.md)

No test suite can create a literal `0%` production-breakage guarantee. What this repo can do is define a release bar that makes silent regressions in execution recording, traceability, and state auditability very hard to ship.

## Subsystem matrix

Every major Flux subsystem should have both a direct owner test and a cross-system test.

| Subsystem | Direct coverage | Cross-system coverage |
|---|---|---|
| CLI | `cargo test -p cli why::tests` + [cli_test.sh](../../scripts/platform-tests/cli_test.sh) | request inspection via `trace`, `why`, `doctor`, `records`, `invoke` |
| Server | [server_test.sh](../../scripts/platform-tests/server_test.sh) | monolith mounts `/health`, `/flux/api`, `/flux/dev/invoke` |
| Runtime request handling | server/router tests + [server_test.sh](../../scripts/platform-tests/server_test.sh) | request entry, readiness, route miss, function invoke |
| Runtime | [runtime_test.sh](../../scripts/platform-tests/runtime_test.sh) | execution path, span/log emission, dispatch integration |
| Queue | crate route tests + [queue_service_test.sh](../../scripts/platform-tests/queue_service_test.sh) | enqueue -> run -> retry/dead-letter -> trace linkage |
| Agents | schema parse tests + [agent_test.sh](../../scripts/platform-tests/agent_test.sh) | deploy, list, get, delete |
| API | middleware tests + [api_test.sh](../../scripts/platform-tests/api_test.sh) | health, records, trace/debug/query surfaces |
| Database dispatch | mutation/history tests + [state_audit_test.sh](../../scripts/platform-tests/state_audit_test.sh) | direct query execution and mutation auditability |
| End-to-end loop | [execution_record_test.sh](../../scripts/platform-tests/execution_record_test.sh) + [state_audit_test.sh](../../scripts/platform-tests/state_audit_test.sh) | invoke -> request id -> trace -> why -> history |

### 1. Unit tests

These protect pure logic and should run on every commit.

Focus areas:

- request-id resolution and precedence
- query guard complexity and depth scoring
- query compiler output for `select`, `insert`, `update`, `delete`
- mutation-history parsing and row-key handling
- trace diffing and JSON field-diff logic
- CLI rendering logic for `trace`, `why`, `doctor`, and `state history`
- retention and pruning helpers
- config resolution precedence for the CLI

Recommended command shape:

```bash
cargo test --workspace
```

### 2. Contract tests

These verify that service boundaries stay stable even when implementations change.

Must cover:

- request handling -> runtime request shape
- runtime -> API log write envelope
- API -> database dispatch auth and forwarded headers
- `/traces`, `/logs`, `/records/*`, `/db/mutations`, `/db/history`, `/db/replay` JSON contracts
- response headers that matter for DX, especially `x-request-id` and `x-cache`

Contract failures are high severity because Flux depends on multiple subsystems telling one coherent story.

### 3. Product-loop end-to-end tests

These are the highest-value tests in the repo. They protect the thing users buy:

- execute one function
- get back a request id
- open the trace for that request
- inspect logs tied to that request
- export the execution record
- mutate state through the data plane
- inspect mutation log, row history, and replay window for that same request

Executable scripts in this repo:

- [scripts/platform-tests/execution_record_test.sh](../../scripts/platform-tests/execution_record_test.sh)
- [scripts/platform-tests/state_audit_test.sh](../../scripts/platform-tests/state_audit_test.sh)

Run just the core product promise:

```bash
make test-product-loop
```

### 4. Platform completeness end-to-end tests

These cover the rest of the “complete runtime” story:

- schema graph and SDK generation
- DB reads and cache behavior
- file URL generation
- function invocation
- logs listing
- events stream availability
- auth protection
- concurrency/load smoke

Run the full shell harness:

```bash
make test-platform
```

## Environment for platform tests

Current shell tests expect:

```bash
API_URL
TOKEN
TENANT_ID
PROJECT_ID
FUNCTION_NAME
DB_URL
RUNTIME_URL
QUEUE_URL
```

Optional mutation-test overrides:

```bash
MUTATION_DATABASE      # defaults to main
MUTATION_TABLE         # defaults to users
MUTATION_PK_FIELD      # defaults to id
MUTATION_INSERT_JSON   # full JSON body for POST /db/query
MUTATION_HISTORY_QUERY # override row lookup query string for /db/history
TRACE_TEST_PAYLOAD     # override function payload for execution-record test
```

Optional service and monolith overrides:

```bash
SERVER_URL                 # explicit monolith base URL; defaults from API_URL
INTERNAL_SERVICE_TOKEN     # shared internal token when services are configured with one value
API_INTERNAL_SERVICE_TOKEN # override token used for /internal/* API tests
FLUX_BIN                   # CLI binary path for cli_test.sh
```

## Release-bar scenarios

The following scenarios define what “safe for beta” means.

### Execution record integrity

Every externally visible request must:

- return an `x-request-id`
- appear in `/traces`
- return a non-empty detail trace from `/traces/{request_id}`
- surface correlated rows in `/logs`
- appear in `/records/export`

### State audit integrity

Every mutation through `/db/query` must:

- write at least one row to `/db/mutations?request_id=...`
- retain correct `before_state` and `after_state`
- increment row history in `/db/history/:database/:table`
- appear in `/db/replay/:database` for its time window
- keep ordering stable via `mutation_seq`

### Debugging UX integrity

Before `0.1 beta`, the following CLI flows need automated coverage:

- `flux trace <request_id>`
- `flux why <request_id>`
- `flux state history <table> --id <pk>`
- `flux trace diff <original> <replay>`
- `flux doctor <request_id>`

Passing criteria:

- commands must not panic
- output must contain the request id or targeted row
- failure paths must remain actionable, not generic

### Queue and background lineage

Because Flux sells a complete system, background work must stay inside the same debugging model.

Required coverage before beta:

- queue publish -> worker execution -> trace linkage
- DLQ replay -> successful re-enqueue semantics
- background-triggered executions produce inspectable traces, not orphan work

### Security and isolation

Required automated checks:

- unauthenticated requests rejected where expected
- internal endpoints reject missing/invalid service token
- project scoping is honored on read surfaces
- replay flags do not trigger side effects outside intended paths
- logs and records never store raw credentials

### Resilience

Before beta, add repeatable suites for:

- process restart with retained DB state
- migration forward on a non-empty database
- replay on large mutation windows
- queue backlog recovery
- trace/list endpoints under concurrent load
- retention/prune jobs on realistic row counts

### Soak and performance

Smoke tests are not enough. Beta should include:

- sustained request load for at least 30 minutes
- sustained mutation load with history queries in parallel
- records export against large log volumes
- queue worker throughput under backlog
- trace and why latency budgets for recent requests

## Test philosophy

Flux should bias toward catching failures at the product boundary instead of only inside crates.

A passing suite should answer:

- Did the request run?
- Was the request recorded?
- Can the record explain the request?
- Can the state change be audited?
- Can background work be traced back to the origin?

If a new feature cannot be validated against those questions, it is not yet integrated into the Flux story.
