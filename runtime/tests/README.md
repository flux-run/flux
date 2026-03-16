# Flux Runtime Test Suite

A runtime compatibility test suite that validates `flux-runtime` across multiple dimensions.

## Suites

The suite is organized into 8 layers. Run them via `node dist/src/cli.js all` after `npm run build`.

| Suite | Tests | Status |
|-------|-------|--------|
| ECMAScript | 52 | mostly passing |
| Node.js APIs | 36 | partial (require-based APIs not available in Deno) |
| Web APIs | 27 | mostly passing |
| Frameworks | 17 | passing |
| Runtime | 21 | passing |
| Determinism | 17 | mostly passing |
| Error Handling | 22 | passing |
| Concurrency | 18 | mostly passing |

**Current: 199/210 passing** (11 known failures — all `require is not defined` in the Node.js compat layer; `flux-runtime` uses Deno's module system).

## What Each Suite Tests

**ECMAScript** — arrow functions, destructuring, template literals, promises, async/await, classes, generators, Map/Set/Symbol, built-in methods.

**Node.js APIs** — `fs`, `events`, `timers`, `buffer`, `path`, `process`, `crypto`. Note: tests that use `require()` fail because `flux-runtime` is ESM-first via Deno.

**Web APIs** — URL/URLSearchParams, Headers/Request/Response, Blob/ArrayBuffer, TextEncoder/TextDecoder, AbortController, atob/btoa.

**Frameworks** — Express/Koa-style routing, middleware chains, route parameters, query strings, async handlers, error handling.

**Runtime** — stress tests: large objects, deep nesting, complex transformation chains, recursive functions, closure correctness.

**Determinism** — promise resolution order, event emission order, iteration order, JSON consistency. Critical for `flux replay` correctness.

**Error Handling** — try/catch/finally, promise rejection, custom error types, re-throwing, fallbacks.

**Concurrency** — parallel promises, mixed timing, microtask vs macrotask ordering, data isolation between concurrent requests.

## Quick Start

```bash
cd runtime/tests
npm install
npm run build
node dist/src/cli.js all
```

Run a specific suite:

```bash
node dist/src/cli.js ecmascript
node dist/src/cli.js node
node dist/src/cli.js web
node dist/src/cli.js frameworks
node dist/src/cli.js runtime
node dist/src/cli.js determinism
node dist/src/cli.js error-handling
node dist/src/cli.js concurrency
```
