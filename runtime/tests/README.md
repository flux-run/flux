# Flux Runtime Test Suite

Comprehensive test coverage for `flux-runtime` — the Deno V8 isolate that executes user functions.

Tests are split into two layers:

| Layer | Location | Purpose |
|-------|----------|---------|
| **Internal suites** | `runtime/tests/` | Authored tests; run in seconds; always green |
| **External / compatibility** | `runtime/runners/` | Official third-party suites (Test262, WPT, Node core) + npm ecosystem |

---

## Internal Suites

### Quick Start

```bash
cd runtime/tests
npm install
npm run build
```

### Flagship suites (compact dashboard output)

These are the suites to run in day-to-day development and CI. Each prints a one-line-per-category summary.

```bash
npm run test:trust    # 40 tests  — JS correctness, isolation, replay magic
npm run test:compat   # 52 tests  — real-world app patterns (Zod, uuid, axios, JWT, …)
npm run test:replay   # 8 tests   — true end-to-end: record → replay → compare outputs
npm run test:modules  # 17 tests  — ESM imports, dynamic import(), circular deps, module cache
```

### General regression suite

```bash
node dist/src/cli.js all           # all 8 general suites at once
node dist/src/cli.js ecmascript    # ES language features
node dist/src/cli.js node          # Node.js built-in API surface
node dist/src/cli.js web           # Web APIs (URL, fetch, TextEncoder, …)
node dist/src/cli.js frameworks    # Express/Koa-style routing patterns
node dist/src/cli.js runtime       # stress: large objects, deep recursion, closures
node dist/src/cli.js determinism   # promise order, iteration order — critical for replay
node dist/src/cli.js error-handling
node dist/src/cli.js concurrency
```

### Full suite inventory

| Suite | Filter | Tests | What it covers |
|-------|--------|-------|----------------|
| **Trust** | `trust` | 40 | Language correctness, Node APIs, Web APIs, isolation, concurrency, error handling, stress, replay |
| **Compatibility** | `compat` | 52 | API handlers, Zod validation, axios HTTP, uuid/crypto, DB & queue workflow mocks, middleware, large payloads, concurrency, replay, isolation, performance |
| **Replay** | `replay` | 8 | `Math.random`, `Date.now`, `fetch` — record values, replay identical values, compare outputs byte-for-byte |
| **Module Loader** | `modules` | 17 | Static named/default imports, barrel re-exports, dynamic `import()`, conditional imports, circular A↔B, module cache singleton |
| ECMAScript | `ecmascript` | ~52 | Arrow fns, destructuring, template literals, async/await, classes, generators, Map/Set/Symbol |
| Node.js APIs | `node` | ~36 | `fs`, `events`, `timers`, `buffer`, `path`, `process`, `crypto` |
| Web APIs | `web` | ~27 | `URL`, `URLSearchParams`, `Headers`, `Request`, `Response`, `Blob`, `TextEncoder`, `AbortController` |
| Frameworks | `frameworks` | ~17 | Routing, middleware chains, route params, query strings, async handlers |
| Runtime | `runtime` | ~21 | Large object throughput, deep nesting, complex chains, recursive fns |
| Determinism | `determinism` | ~17 | Promise resolution order, event emission order, iteration order, JSON consistency |
| Error Handling | `error-handling` | ~22 | try/catch/finally, promise rejection, custom error types, re-throwing |
| Concurrency | `concurrency` | ~18 | Parallel promises, microtask vs macrotask ordering, data isolation |

**Total internal tests: ~329**

---

## Suites in Detail

### Trust Suite (`npm run test:trust`)

The primary signal for "does Flux's JavaScript engine work correctly?" — 40 high-signal tests grouped into 8 categories.

| Category | Tests | What fails if broken |
|----------|-------|----------------------|
| Language | 7 | Core JS syntax and built-ins are broken |
| Node APIs | 6 | `Buffer`, `crypto`, `events`, `path` are unavailable |
| Web APIs | 5 | `URL`, `TextEncoder`, `fetch` are missing |
| Isolation | 4 | Globals leak between isolate invocations |
| Concurrency | 4 | Promises race incorrectly under load |
| Error Handling | 5 | Errors swallowed or formatted wrongly |
| Stress | 4 | Memory or CPU limits cause incorrect results |
| Replay | 5 | `flux replay <id>` produces different output |

### Compatibility Suite (`npm run test:compat`)

Proves that real application code patterns work — not just micro-benchmarks.

| Category | Tests | Libraries / patterns |
|----------|-------|----------------------|
| API Handlers | 4 | Request parsing, content negotiation, streaming |
| Validation | 6 | Zod schemas, error formatting, safe parse |
| HTTP Clients | 5 | axios instances, interceptors, error handling |
| UUID / Crypto | 5 | `uuid` v4/v5, `crypto.randomBytes`, HMAC |
| DB Workflow | 5 | Insert, read, update, error rollback (mock) |
| Queue Workflow | 4 | Enqueue, dequeue, retry logic (mock) |
| Middleware | 4 | Auth, rate-limit, logging middleware chains |
| Large Payloads | 4 | 1 MB JSON, deep nesting, array bulk ops |
| Concurrency | 4 | Parallel DB calls, fan-out aggregation |
| Replay | 5 | Idempotent handlers, deterministic output |
| Isolation | 4 | Cross-request state isolation |
| Performance | 2 | Throughput baseline assertions |

### Replay Suite (`npm run test:replay`)

Proves Flux's core guarantee end-to-end at the unit level.

```
Record  →  handler runs; Recorder captures Math.random, Date.now, fetch responses
Replay  →  same handler runs; Replayer injects captured values in order
Assert  →  output is byte-for-byte identical to original
```

| Test | Non-deterministic source covered |
|------|----------------------------------|
| Math.random: single call | RNG |
| Date.now: timestamp | Clock |
| Multiple Math.random calls | RNG sequence order |
| Mixed random + timestamp | Both sources together |
| fetch: single request | Network I/O |
| Multiple fetches | Per-call response assignment |
| Pure handler | No recording needed; confirms stability |
| Full execution snapshot | All sources; asserts entire log consumed |

### Module Loader Suite (`npm run test:modules`)

Exercises the ESM module system — patterns that break new runtimes.

| Category | Tests | What is verified |
|----------|-------|-----------------|
| Static named imports | 2 | Constants and functions from a `.ts` fixture |
| Static default imports | 2 | Class instantiation; independent instances |
| Barrel re-exports | 3 | Named value, function, default-as-named forwarding |
| Dynamic `import()` | 4 | Lazy load, conditional, `.default`, repeated = same ref |
| Circular imports | 4 | A→B→A live bindings; no TDZ errors; cross-calls work |
| Module cache | 2 | Top-level initializer runs exactly once; singleton stable |

---

## Integration Tests (real binary)

These tests compile the real Flux binaries and verify them end-to-end through the same user-facing commands developers run locally. Some suites start HTTP handlers with `flux run --listen`; others execute scripts directly with `flux run`. Together they exercise the full stack: Cargo build → CLI start → runtime load → request handling or script execution.

> **Why this matters:** the internal suites (trust / compat / replay / modules) run JavaScript assertions inside a Node.js process. They prove the JS logic is correct but never touch the Rust binaries. Integration tests close that gap.

### Quick start

```bash
# From runtime/tests/ (builds binary automatically if missing)
npm run test:integration

# Skip the cargo build step if the binary is already up-to-date
npm run test:integration:skip-build

# Or run directly from runtime/runners/
cd runtime/runners
npm run test:integration
npm run test:integration:skip-build
npm run test:integration -- --suite echo   # one suite only
```

### What it tests

HTTP suites start a `flux run --listen` process on a unique port (3100–3199), wait for the runtime to reach its boot and ready phases, send real requests, assert responses, then stop the process. Command suites execute `flux run` directly and assert the emitted JSON output.

| Suite | Handler | Checks |
|-------|---------|--------|
| `echo` | `echo.js` | Body reflection, field uppercasing, invalid-JSON 400 |
| `json-types` | `json-types.js` | null, bool, int, float, string, array, nested object, UTF-8 |
| `web-apis` | `web-apis.js` | `crypto.randomUUID`, `Date`, `URL`, `TextEncoder`, `btoa/atob`, `structuredClone` |
| `request-isolation` | `request-isolation.js` | request-scoped globals reset between HTTP executions |
| `async-ops` | `async-ops.js` | `await`, `Promise.all`, `Promise.race`, `setTimeout`, sequential pipeline |
| `error-handling` | `error-handling.js` | 404/400/422 explicit responses, sync throw → 5xx, async reject → 5xx |
| `crud-replay` | `examples/crud_app/main_flux.ts` | HTTP CRUD flow plus `flux replay --diff` verification |
| `db-then-remote-resume` | `examples/db_then_remote/main_flux.ts` | failed DB checkpoint is reused and `flux resume` completes remote delivery without duplicate inserts |
| `drizzle-crud` | `examples/drizzle/crud.ts` | direct `flux run`, Drizzle insert/select/update against disposable Postgres |
| `drizzle-transaction` | `examples/drizzle/transaction.ts` | direct `flux run`, Drizzle transaction semantics against disposable Postgres |
| `drizzle-replay` | `examples/drizzle/crud.ts` | recorded `flux run` plus `flux replay --diff` verification for Drizzle + Postgres |

HTTP handler files live in `runtime/external-tests/flux-handlers/`. They use `Deno.serve(...)` so they are valid `flux run --listen` entry targets. The Drizzle suites run the checked-in examples from `examples/drizzle/` after installing their local `node_modules` with `npm ci` when needed.

Listener registration is now a boot-only operation: the runtime allows `Deno.serve(...)` during the boot execution, then closes registration before request executions begin.

When a command suite is executed with a Flux server URL and token, `flux run` prints an `execution_id:` line before the final JSON payload. Integration replay suites capture that ID and assert the stored execution can be replayed deterministically.

### Binary management

`runtime/runners/lib/flux-binary.ts` is the helper that:
- Locates the CLI, runtime, and server binaries under `target/debug/` (or `target/release/` when `FLUX_RELEASE=1`)
- Builds via `SQLX_OFFLINE=true cargo build -p cli -p runtime -p server` when the binaries are missing
- Spawns `flux run --listen` with `--host`, `--port`, and `--isolate-pool-size` for HTTP suites
- Exercises the runtime's explicit boot phase before the listener reaches `[ready]`
- Polls the port every 100 ms until the isolate is ready (15 s timeout)
- Kills spawned processes after each suite (`SIGTERM → SIGKILL`)

### Skipping the build

If you have already built the binary (e.g. via `make build`), pass `--skip-build`:

```bash
npm run test:integration:skip-build
```

Set `FLUX_RELEASE=1` to test against the release binary:

```bash
FLUX_RELEASE=1 npm run test:integration:skip-build
```

---

## External & Ecosystem Compatibility

These suites run official upstream test corpora against the same V8 engine. They live in `runtime/runners/` and require a one-time setup step.

```
runtime/
  external-tests/
    test262/          ← TC39 ECMAScript conformance (~47 000 tests)
    node-core/        ← node/test/parallel + sequential
    web-platform/     ← url/ fetch/ encoding/ WPT subset
    npm/              ← axios · zod · uuid · jsonwebtoken · lodash integration tests
  runners/
    run-test262.ts
    run-node-tests.ts
    run-web-tests.ts
    run-npm-tests.ts
    generate-report.ts
  reports/
    test262.json
    node-tests.json
    web-tests.json
    npm-tests.json
    summary.md        ← generated Markdown compatibility page
  scripts/
    setup-external-tests.sh
```

### One-time setup

```bash
# Clone all three external repos (≈ 200 MB total)
bash runtime/scripts/setup-external-tests.sh all

# Or clone individually:
bash runtime/scripts/setup-external-tests.sh 262    # test262 only
bash runtime/scripts/setup-external-tests.sh node   # node-core only
bash runtime/scripts/setup-external-tests.sh wpt    # web-platform only
```

### Running external suites

```bash
cd runtime/runners
npm install

npm run test:npm     # 68 npm ecosystem tests — no clone needed
npm run test:262     # ECMAScript Test262 — requires test262 clone
npm run test:node    # Node.js core tests — requires node-core copy
npm run test:web     # Web Platform Tests — requires wpt clone
npm run report       # regenerate runtime/reports/summary.md
npm run test:all     # everything + summary
```

#### Forwarding scripts (from runtime/tests/)

```bash
npm run test:npm         # → cd ../runners && npm run test:npm
npm run test:262        # → cd ../runners && npm run test:262
npm run test:node-compat # → cd ../runners && npm run test:node
npm run test:web-compat  # → cd ../runners && npm run test:web
npm run test:report      # → cd ../runners && npm run report
npm run test:compat-all  # → cd ../runners && npm run test:all
npm run test:integration           # → cd ../runners && npm run test:integration
npm run test:integration:skip-build # → cd ../runners && npm run test:integration:skip-build
npm run setup:external   # → bash ../scripts/setup-external-tests.sh all
```

### NPM Ecosystem results (current)

These run in CI without any external clone.

| Library | Tests | Pass Rate |
|---------|-------|-----------|
| axios | 10 | 100% |
| zod | 16 | 100% |
| uuid | 11 | 100% |
| jsonwebtoken | 10 | 100% |
| lodash | 21 | 100% |
| **Total** | **68** | **100%** |

### Compatibility report

After running the external suites, generate a Markdown summary:

```bash
cd runtime/runners && npm run report
# → runtime/reports/summary.md
```

Example output:

```
## Summary
| Suite                | Pass Rate |
|----------------------|-----------|
| ECMAScript (Test262) | 99.1%     |
| Node.js Core Tests   | 63.4%     |
| Web Platform Tests   | 97.8%     |
| NPM Ecosystem        | 100.0%    |
```

---

## CI

GitHub Actions workflow: [`.github/workflows/runtime-tests.yml`](../../.github/workflows/runtime-tests.yml)

Runs on every push / PR to `runtime/**`.

| Job | What it runs |
|-----|-------------|
| `internal-suites` | trust · compat · replay · modules |
| `npm-ecosystem` | 68 npm library tests |
| `test262` | first 2 000 Test262 tests (full run optional) |
| `web-platform` | url · fetch · encoding WPT |
| `summary` | downloads all reports, generates `summary.md`, posts to job summary |

---

## Project Layout

```
runtime/tests/
  src/
    cli.ts          ← dispatcher: routes filter arg to suite runners
    harness.ts      ← TestHarness class: test(), run(), result types
    utils.ts        ← createTestSummary(), formatTestResults()
  suites/
    trust/          ← 40 tests (8 categories)
    compatibility/  ← 52 tests (12 categories)
    replay/         ← 8 end-to-end record→replay tests
    module-loader/  ← 17 ESM module system tests
      fixtures/     ← math.ts, greeter.ts, barrel.ts, circular-a/b.ts, singleton.ts
    ecmascript/     ← ES language features
    node/           ← Node.js built-in APIs
    web/            ← Web APIs
    frameworks/     ← routing / middleware patterns
    runtime/        ← stress tests
    determinism/    ← ordering guarantees
    error-handling/ ← error propagation
    concurrency/    ← async correctness
  package.json
  tsconfig.json
```

## Adding a Test

1. Open the relevant suite file in `suites/<name>/index.ts`.
2. Call `suite.test("description", async () => { … })` inside the `createXxxSuite` function.
3. Throw (or call `assert` / `assertEquals` from `../../src/harness.js`) on failure; return normally to pass.
4. Rebuild: `npm run build` (or `npm run test:watch` for incremental).
5. Run: `npm run test:trust` / `npm run test:compat` / etc.

## Adding a Suite

1. Create `suites/<name>/index.ts` exporting `createXxxSuite(): TestHarness`.
2. Import it in `src/cli.ts` and add a `runSingleSuite(...)` branch for the new filter keyword.
3. Add `"test:<name>": "node dist/src/cli.js <name>"` to `package.json` scripts.
