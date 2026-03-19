# Flux Library Compatibility

> **Core principle:** Flux compatibility is defined by whether a library's side effects
> pass through Flux-controlled boundaries — not whether the library runs.
>
> A library that runs but bypasses Flux boundaries is **more dangerous than one that fails outright**.
> It silently breaks replay guarantees without any error.

---

## How Flux Compatibility Works

Flux intercepts IO at specific **boundaries**. When a library uses a boundary Flux controls,
every operation is checkpointed and replayable. When a library bypasses those boundaries,
operations are invisible to Flux — they happen twice on replay, produce different results,
or corrupt state silently.

```
Flux-controlled boundaries:
  ✅ fetch (Web Fetch API)       — intercepted at the runtime level
  ✅ flux:pg                     — native Postgres driver with full checkpoint support
  ✅ flux:redis                  — native Redis driver with per-command checkpointing
  ✅ crypto.subtle / randomUUID  — deterministic via runtime patching
  ✅ Date.now / Math.random      — deterministic via runtime patching

NOT controlled by Flux:
  ❌ raw TCP sockets             — bypass fetch/pg/redis interception
  ❌ undici's own connection pool — manages its own TCP, invisible to Flux
  ❌ postgres.js own TCP client  — same: manages its own connection outside flux:pg
  ❌ filesystem writes           — not checkpointable; would execute twice on replay
  ❌ child processes             — non-deterministic, outside the sandbox
```

---

## Compatibility Tiers

### ✅ Tier 1: Replay-safe — Flux guarantees fully preserved

All IO passes through Flux-controlled boundaries. Operations are checkpointed, replayed
correctly, and deterministic across executions.

| Library | Boundary Used | Notes |
|---|---|---|
| `fetch` (native) | Flux Web Fetch API | Zero warnings. The reference implementation. |
| `pg` via `flux:pg` | Flux native Postgres | Runtime-injected driver. Full checkpoint coverage. |
| `drizzle-orm` (via `flux:pg`) | Flux native Postgres | ORM layer over `flux:pg`. Fully safe. |
| `redis` (node-redis v4) via `flux:redis` | Flux native Redis | Per-command checkpointing. Blocked commands enforced. |
| `hono` | No IO | Pure router/middleware. Fully compatible. |
| `jose` | `crypto.subtle` (Flux) | Crypto ops use Web Crypto API, controlled by Flux. |
| `zod` | None | Pure computation. No IO. Deterministic by definition. |

> **Note on `flux:*` warnings in `flux check`:**
> `flux check` is a static analyzer. It cannot resolve `flux:pg` and `flux:redis` specifiers
> because these modules are injected by the runtime, not fetched from a CDN. Any `load_failed`
> errors for `flux:*` specifiers are **expected and do not affect runtime correctness**. The
> runtime resolves them correctly at execution time.

---

### ⚠️ Tier 2: Works with caveats — Flux guarantees partially preserved

These libraries run correctly, but their internal implementation references browser or Node.js
globals that are never actually reached in a Flux execution. The referenced code paths (DOM
access, `navigator`, `document`) are dead code in a server context. Operations that pass
through `fetch` or `flux:*` remain replay-safe.

| Library | Boundary Used | Caveat |
|---|---|---|
| `axios` | `fetch` (internally) | Adapts to non-browser env. The actual HTTP calls use `fetch` → intercepted by Flux. Internal browser globals (`document`, `navigator`) are dead code in server context. **Replay-safe for HTTP calls.** |
| `ioredis` | `flux:redis` (via routing) | If routed through the Flux Redis driver: replay-safe. If used with a raw TCP connection to Redis: **NOT replay-safe.** Use the `flux:redis` import. |
| `postgres.js` (`npm:postgres`) | Own TCP client | Manages its own connection pool outside `flux:pg`. **NOT replay-safe.** Queries execute twice on replay. Use `flux:pg` instead. |

> ⚠️ **postgres.js warning:** This library bypasses Flux's Postgres interception layer.
> It will appear to work but queries **will not be checkpointed**. On replay, every query
> executes against the live database again. Use `import pg from "flux:pg"` instead.

---

### ❌ Tier 3: Breaks deterministic execution model

> ⚠️ These libraries **run without errors** but **silently break Flux's execution guarantees**.
> This is more dangerous than an outright error. Replay will produce different results,
> duplicate side effects, or corrupt state — without any warning.

| Library | Why it breaks | Correct alternative |
|---|---|---|
| `undici` | Manages its own connection pool at the TCP level. HTTP calls are invisible to Flux — not checkpointed, not replayable. On replay, every request fires again against the real server. | Use native `fetch` |
| Any raw TCP/socket client | Bypasses Flux's network interception layer entirely. | Use `fetch`, `flux:pg`, or `flux:redis` |

**The undici problem in detail:**

```
What you expect:  fetch("https://api.example.com") → checkpointed → replay suppresses call
What undici does: undici manages its own socket pool → NOT checkpointed → replay fires AGAIN
Result:           duplicate API calls, non-deterministic state, silent guarantee violation
```

---

## `flux check` Output Interpretation

```
✅  Compatible     — no unsupported dependencies detected
⚠️  Warning        — globals/browser APIs present but NOT necessarily called at runtime
❌  Incompatible   — known structural issues that break Flux's interception model
```

| Warning type | Meaning | Action needed? |
|---|---|---|
| `unsupported_global: process` | Library may reference Node.js `process` | Usually a dead code path. Test at runtime. |
| `unsupported_global: Buffer` | Library may reference Node.js `Buffer` | Usually a dead code path. Test at runtime. |
| `unsupported_web_api: navigator` | Library may reference browser `navigator` | Dead code in server context. Safe to ignore. |
| `unsupported_web_api: Worker` | Library uses Web Workers | May cause issues. Test carefully. |
| `load_failed: flux:*` | Static checker cannot resolve Flux native specifier | **Expected.** Runtime resolves correctly. Ignore. |

---

## Quick Reference

```
Use this:                      Instead of:
─────────────────────────────────────────────
import pg from "flux:pg"       npm:pg / postgres.js
import { createClient }
  from "flux:redis"            npm:ioredis / npm:redis (direct TCP)
fetch(...)                     import { request } from "undici"
```

---

## Tested Versions

Results from `flux check` — `flux 0.1.2` — run 2026-03-19.

| Library | Version | Modules | `flux check` verdict |
|---|---|:---:|---|
| `fetch` (native) | — | 18 | ✅ Clean |
| `axios` | 1.13.6 | 44 | ⚠️ Warnings (dead browser code) |
| `undici` | 7.24.4 | 57 | ❌ Breaks guarantees (own TCP) |
| `pg` via `flux:pg` | 8.20.0 | 18 | ✅ Clean (flux: expected) |
| `drizzle-orm` | latest | 183 | ✅ Clean (flux: expected) |
| `zod` | 4.3.6 | 33 | ⚠️ Warnings (dead process refs) |
| `jose` | 6.2.2 | 67 | ⚠️ Warnings (JWKS dead path) |
| `ioredis` | 5.10.1 | 69 | ⚠️ Warnings (Buffer/process) |
| `redis` (node-redis) | v4 via `flux:redis` | 18 | ✅ Clean (flux: expected) |
| `postgres.js` | 3.4.8 | 39 | ⚠️ Breaks guarantees (own TCP) |
