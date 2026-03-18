# Flux Runtime Compatibility Report

> Generated: 2026-03-16

## Summary

| Suite | Passed | Failed | Skipped | Pass Rate | Status |
|-------|--------|--------|---------|-----------|--------|
| ECMAScript (Test262) | — | — | — | N/A | ⚪ not run |
| Node.js Core Tests | — | — | — | N/A | ⚪ not run |
| Web Platform Tests | — | — | — | N/A | ⚪ not run |
| NPM Ecosystem | 68 | 0 | 0 | 100.0% | 🟢 |

---

## ECMAScript — Test262

_Not run. Clone test262 and run `npm run test:262`._

```bash
cd runtime/external-tests
git clone --depth 1 https://github.com/tc39/test262 test262
cd ../runners && npm run test:262
```

---

## Node.js Core Tests

_Not run. Copy node/test/parallel and run `npm run test:node`._

```bash
bash runtime/scripts/setup-external-tests.sh node
cd runtime/runners && npm run test:node
```

---

## Web Platform Tests

_Not run. Sparse-clone wpt and run `npm run test:web`._

```bash
bash runtime/scripts/setup-external-tests.sh wpt
cd runtime/runners && npm run test:web
```

---

## NPM Ecosystem

| Library | Pass Rate | Status |
|---------|-----------|--------|
| `axios` | 100.0% (10/10) | ✅ |
| `zod` | 100.0% (16/16) | ✅ |
| `uuid` | 100.0% (11/11) | ✅ |
| `jsonwebtoken` | 100.0% (10/10) | ✅ |
| `lodash` | 100.0% (21/21) | ✅ |

---

_Flux runtime uses Deno V8 isolates. ECMAScript compliance reflects the V8 engine._
_Node.js API compatibility reflects Deno's Node.js compatibility layer._
