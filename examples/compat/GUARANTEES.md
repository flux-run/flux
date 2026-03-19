# Flux Execution Guarantees

This file maps every compat test to the Flux execution law(s) it validates.

> **Definition:** A "Flux guarantee" is a property that holds across ALL executions,
> including on replay — not just on first run. If a property cannot be guaranteed
> on replay, it is not a Flux guarantee.

---

## The 5 Execution Laws

| Law | Definition |
|---|---|
| **DETERMINISM** | Non-random sources (UUID, Date.now, Math.random) are patched. Same inputs → same outputs on replay. |
| **REPLAY SAFETY** | On replay, IO side effects are suppressed. No second HTTP call, no second DB write. |
| **ISOLATION** | No mutable state leaks between executions. Module-level globals reset per isolate. |
| **ORDERED IO** | Checkpoints are assigned monotonic indexes. Replay maps call_index → recorded result. |
| **BOUNDARY BLOCK** | Unsupported features (non-deterministic IO) fail with an explicit contract error. Never silently succeed. |

---

## Test File → Law Coverage Matrix

| Test File | DETERMINISM | REPLAY SAFETY | ISOLATION | ORDERED IO | BOUNDARY BLOCK |
|---|:---:|:---:|:---:|:---:|:---:|
| `flux-invariants.ts` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `redis-contract.ts` | | ✅ | | | ✅ |
| `ioredis-compat.ts` | | | | ✅ | ✅ |
| `redis-compat.ts` | | | | ✅ | ✅ |
| `fetch-compat.ts` | ✅ | ✅ | | ✅ | |
| `axios-compat.ts` | | ✅ | | ✅ | |
| `undici-compat.ts` | | ✅ | | ✅ | |
| `pg-compat.ts` | | ✅ | ✅ | ✅ | |
| `drizzle-compat.ts` | | ✅ | ✅ | ✅ | |
| `jose-compat.ts` | ✅ | | | | |
| `zod-compat.ts` | ✅ | | | | |

---

## What Flux Guarantees Per Library

### fetch / axios / undici

```
✅ All outbound HTTP calls are checkpointed before the response is consumed
✅ On replay, the recorded response is returned without a real network request
✅ Errors (timeout, connection refused, 4xx/5xx) are recorded and replayed identically
✅ Concurrent outbound calls are all checkpointed (order may vary, but each is captured)
```

### pg (node-postgres) / Drizzle ORM

```
✅ Every SQL statement is checkpointed (SELECT, INSERT, UPDATE, DELETE, DDL)
✅ On replay, recorded query results are returned without touching the database
✅ Transaction commit/rollback is fully recorded — replayed exactly
✅ Constraint violations (23505, 23502, 42601) are recorded and replayed
✅ Concurrent queries are all checkpointed independently
```

### Redis (via flux:redis and flux:ioredis)

```
✅ Stateless, per-command operations are checkpointed (see supported command list)
✅ On replay, recorded command results are returned without contacting Redis
✅ Command errors (wrong type, key missing) are recorded and replayed identically

❌ MULTI / EXEC / WATCH / UNWATCH  → blocked: non-deterministic session state
❌ SUBSCRIBE / PSUBSCRIBE / PUBLISH → blocked: pub/sub requires persistent connection
❌ BLPOP / BRPOP / BZPOPMIN / BZPOPMAX → blocked: blocking commands cannot be replayed
❌ XREAD (with BLOCK) / XREADGROUP (with BLOCK) → blocked: blocking stream reads
```

**Error format when blocked:**
```
Redis {feature} are not supported in Flux (non-deterministic execution)
```

### jose / webcrypto

```
✅ crypto.subtle.sign / verify / digest / deriveBits / generateKey / importKey / exportKey
✅ crypto.randomUUID() — deterministic on replay (recorded value returned)
✅ PBKDF2, ECDSA (ES256/ES384), RSASSA-PKCS1-v1_5, HMAC-SHA256 all supported
✅ JWT sign/verify via npm:jose fully supported
```

### Zod

```
✅ Pure computation — no IO. All validation is deterministic.
✅ safeParse / parse / transform / refine all work identically on replay.
```

---

## What Flux Does NOT Guarantee

These are explicit non-guarantees. If your code relies on any of these, replay is undefined:

| Behavior | Reason |
|---|---|
| Local filesystem writes (`Deno.writeTextFile`, `fs.writeFile`) | Cannot be checkpointed. Replay would write twice. |
| Child processes (`Deno.Command`, `child_process.spawn`) | Non-deterministic, not checkpointable. |
| Unix domain sockets | Bypass TCP interception layer. |
| Redis sessions (`MULTI/EXEC/WATCH`) | Session state cannot be reconstructed from individual checkpoints. |
| Redis pub/sub (`SUBSCRIBE/PUBLISH`) | Requires persistent connection; not per-command. |
| Redis blocking commands (`BLPOP/BRPOP`) | Would block the runtime — not replayable. |
| Native addons (`.node` / WASM outside V8) | Execute outside the sandbox. |
| Long-lived WebSocket connections | Not checkpointable at the message level (yet). |

---

## How to Run Replay Proofs

The following routes are specifically designed to be verified via `flux replay`:

```bash
# 1. Make a request that creates a DB row
curl -X POST http://localhost:3000/replay-proof/insert  \
  -H 'content-type: application/json' \
  -d '{"label":"test-replay"}' \
  -v  # note x-flux-execution-id

# 2. Replay it — the DB write is suppressed, response is identical
flux replay <execution-id> --diff

# 3. Confirm only 1 row exists (not 2)
psql -c "SELECT COUNT(*) FROM flux_replay_proof WHERE label='test-replay'"

# 4. Cleanup
curl -X POST http://localhost:3000/replay-proof/cleanup
```

---

## Canary Suite

`flux-invariants.ts` is the **system canary**. It must pass completely after every runtime change.

Any test failure in this file means a Flux execution law has been violated:

| Route | Law |
|---|---|
| `/determinism/uuid` | DETERMINISM |
| `/determinism/date` | DETERMINISM |
| `/determinism/math-random` | DETERMINISM |
| `/determinism/crypto-digest` | DETERMINISM |
| `/replay-proof/insert` | REPLAY SAFETY |
| `/replay-proof/http` | REPLAY SAFETY |
| `/isolation/module-state` | ISOLATION |
| `/isolation/no-shared-closure` | ISOLATION |
| `/isolation/concurrent-db` | ISOLATION |
| `/ordered-io/sequential` | ORDERED IO |
| `/ordered-io/mixed` | ORDERED IO |
| `/ordered-io/concurrent` | ORDERED IO |
| `/boundary/filesystem-write` | BOUNDARY BLOCK |
| `/boundary/child-process` | BOUNDARY BLOCK |
| `/integration/idempotent-create` | REPLAY SAFETY + DETERMINISM + ORDERED IO |

**DO NOT BREAK THIS SUITE.** Any underlying runtime change must pass this benchmark completely.
