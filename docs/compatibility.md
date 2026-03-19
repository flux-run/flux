# Compatibility Guide

> **Core principle:** Flux compatibility is defined by whether a library's side effects
> pass through **Flux-controlled boundaries** — not whether the library runs.
>
> A library that runs but bypasses Flux boundaries is **more dangerous than one that fails outright** —
> it silently breaks replay guarantees without any error.

---

## How Flux Compatibility Works

Flux intercepts IO at specific boundaries. When a library routes through a Flux-controlled boundary,
every operation is checkpointed and replayable. When a library bypasses those boundaries,
operations are invisible to Flux — they execute again on replay, producing duplicate side effects
or non-deterministic state.

```
✅ Flux-controlled boundaries:
   fetch (Web Fetch API)         — intercepted at the runtime level
   flux:pg                       — native Postgres driver, full checkpoint coverage
   flux:redis                    — native Redis driver, per-command checkpointing
   crypto.subtle / randomUUID    — deterministic via runtime patching
   Date.now / Math.random        — deterministic via runtime patching

❌ NOT controlled by Flux:
   raw TCP sockets               — bypass TCP interception layer
   undici's own connection pool  — manages its own TCP; invisible to Flux
   postgres.js own TCP client    — same: manages its own connection outside flux:pg
   filesystem writes             — not checkpointable; would execute twice on replay
   child processes               — non-deterministic, outside the sandbox
```

---

## Compatibility Tiers

### ✅ Tier 1: Replay-safe — Flux guarantees fully preserved

All IO passes through Flux-controlled boundaries. Operations are checkpointed, replayed
without re-executing live calls, and deterministic across executions.

| Library | Boundary | Notes |
|---|---|---|
| `fetch` (native) | Flux Web Fetch API | Zero warnings. The reference implementation. |
| `pg` via `flux:pg` | Flux native Postgres | Runtime-injected driver. Full checkpoint coverage. |
| `drizzle-orm` (via `flux:pg`) | Flux native Postgres | ORM layer over `flux:pg`. Fully safe. |
| `redis` (node-redis v4) via `flux:redis` | Flux native Redis | Per-command checkpointing. Blocked commands enforced. |
| `hono` | No IO | Pure router/middleware. Fully compatible. |
| `jose` | `crypto.subtle` (Flux) | Crypto ops use Web Crypto API, controlled by Flux. |
| `zod` | None | Pure computation. No IO. Deterministic by definition. |

> **Note on `flux:*` warnings in `flux check`:**
> These modules are injected by the runtime, not fetched from a CDN. Any `load_failed` errors
> for `flux:*` specifiers are **expected and do not affect runtime correctness**.

---

### ⚠️ Tier 2: Works with caveats

These libraries run correctly, but have internal references to browser or Node.js globals
that are never reached in a Flux execution. The IO that matters routes through safe boundaries.

| Library | Boundary | Caveat |
|---|---|---|
| `axios` | `fetch` (internally) | HTTP calls route through `fetch` → intercepted by Flux. Internal browser globals (`document`, `navigator`) are dead code in server context. **Replay-safe for HTTP calls.** |
| `ioredis` | `flux:redis` (via routing) | Safe when routed through Flux Redis driver. Not safe with a raw TCP connection to Redis. |
| `postgres.js` | Own TCP client | Manages its own connection pool outside `flux:pg`. Queries execute twice on replay. | 

> ⚠️ **postgres.js:** This library bypasses Flux's Postgres interception layer. Use `import pg from "flux:pg"` instead.

---

### ❌ Tier 3: Breaks execution guarantees

> These libraries **run without errors** but **silently break Flux's execution guarantees**.
> Replay will produce different results, duplicate side effects, or corrupt state — without warning.

| Library | Why it breaks | Correct alternative |
|---|---|---|
| `undici` | Manages its own connection pool at the TCP level. HTTP calls are not checkpointed. On replay, every request fires again against the real server. | Use native `fetch` |
| Any raw TCP/socket client | Bypasses Flux's network interception layer entirely. | Use `fetch`, `flux:pg`, or `flux:redis` |

**The undici failure mode:**
```
Expected: fetch("https://api.example.com") → checkpointed → replay suppresses call
Reality:  undici manages its own socket → NOT checkpointed → replay fires AGAIN
Result:   duplicate API calls, non-deterministic state, silent guarantee violation
```

---

## `flux check` Output Guide

```
✅  Compatible     — no unsupported dependencies detected
⚠️  Warning        — globals/APIs present but NOT necessarily called at runtime
❌  Incompatible   — known structural issues that break Flux's interception layer
```

| Warning type | Meaning | Action |
|---|---|---|
| `unsupported_global: process` | Library may reference Node.js `process` | Usually dead code. Test at runtime. |
| `unsupported_global: Buffer` | Library may reference Node.js `Buffer` | Usually dead code. Test at runtime. |
| `unsupported_web_api: navigator` | Library may reference browser `navigator` | Dead code in server context. Safe to ignore. |
| `load_failed: flux:*` | Static checker cannot resolve Flux native specifier | **Expected behavior.** Runtime resolves correctly. Ignore. |

---

## Supported Web APIs

The following APIs are available in the global scope:

- **Fetch API**: `fetch()`, `Request`, `Response`, `Headers` — intercepted and checkpointed
- **Crypto**: `crypto.getRandomValues()`, `crypto.randomUUID()` (deterministic), `crypto.subtle.*`
- **Timers**: `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`, `queueMicrotask`
- **Encoding**: `TextEncoder`, `TextDecoder`, `btoa`, `atob`
- **Streams**: `ReadableStream` (buffered), `WritableStream`, `TransformStream`
- **URL**: `URL`, `URLSearchParams`
- **Determinism**: `Date`, `performance.now()`, `Math.random()` — all patched for replay stability
- **Console**: Fully supported

---

## What is NOT Supported

| Feature | Reason |
|---|---|
| Filesystem writes (`Deno.writeTextFile`, `fs.writeFile`) | Cannot be checkpointed. Replay would write twice. |
| Child processes (`Deno.Command`, `child_process.spawn`) | Non-deterministic, not checkpointable. |
| Redis sessions (`MULTI/EXEC/WATCH`) | Session state cannot be reconstructed from individual checkpoints. |
| Redis pub/sub (`SUBSCRIBE/PUBLISH`) | Requires persistent connection; not per-command. |
| Redis blocking commands (`BLPOP/BRPOP`) | Would block the runtime — not replayable. |
| Native addons (`.node` / WASM outside V8) | Execute outside the sandbox. |
| Unix domain sockets | Bypass TCP interception layer. Use TCP connections. |

---

## The Golden Path

This is the fully-tested, fully-safe stack. Everything here is covered by the
execution contract test suite (`examples/compat/flux-contract-suite.ts`).

```ts
import { Hono } from "npm:hono"           // ✅ router
import pg from "flux:pg"                   // ✅ postgres (Flux-native)
import { createClient } from "flux:redis"  // ✅ redis (Flux-native)
import { z } from "npm:zod"               // ✅ validation
import * as jose from "npm:jose"           // ✅ JWT / crypto

// fetch() is globally available — no import needed
// crypto.subtle / crypto.randomUUID() are globally available and deterministic
```
