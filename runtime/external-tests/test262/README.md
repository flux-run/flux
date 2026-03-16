# ECMAScript Test262

This directory holds the official TC39 Test262 conformance suite.

## Setup

```bash
# from the repo root
cd runtime/external-tests
git clone --depth 1 https://github.com/tc39/test262 test262
```

The runner requires at minimum:
```
test262/
  harness/    ← polyfills injected before each test
  test/       ← > 47 000 test cases
```

## What is tested

Test262 validates ECMAScript language compliance at the spec level. Because
Flux executes user functions inside Deno V8 isolates (the same V8 engine that
powers both Chrome and Node.js) the test262 results directly reflect the
ECMAScript compliance of the JavaScript engine Flux uses at runtime.

## Running

```bash
cd runtime/runners
npm run test:262
```

Results are written to `runtime/reports/test262.json`.

## Exclusions

The runner automatically skips:
- `test/annexB/` — legacy browser syntax
- `test/intl402/` — ICU-dependent Intl tests (optional engine feature)
- Any test with `flags: [module]` (Flux user code is treated as CommonJS scripts)
