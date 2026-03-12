# Observability & Debugging

Flux automatically instruments every request end-to-end. No code changes,
no SDK calls, no OpenTelemetry setup. Every function execution produces an
[Execution Record](concepts.md#execution-record) containing spans, database
mutations, external calls, and timing — all linked by `request_id`.

---

## Distributed tracing

Every request routed through the gateway receives a unique `x-request-id`
(UUID). Spans are written to the `execution_spans` table as the request flows
through each service:

| Source | What is instrumented |
|---|---|
| Gateway | Route match, auth, rate limit, cache hit/miss |
| Runtime | Bundle cache, execution start/end, tool calls |
| Function | `ctx.log.info()` / `ctx.log.warn()` / `ctx.log.error()` calls |
| Data Engine | Every DB query — table, duration, row count |
| Queue | Job enqueue, worker pickup, execution, retry |

### Reading a trace

```bash
# Get request-id from response header
curl -D - http://localhost:4000/create_user \
  -H "Content-Type: application/json" \
  -d '{"name":"Ada","email":"ada@acme.com"}' 2>&1 | grep x-request-id

# Display the full trace
flux trace a3f9d2b1-4c8e-4f7d-b2e1-9d0c3a5f8e2b
```

```
Trace a3f9d2b1-...  24ms end-to-end

  09:41:02.000  +0ms   ▶ [gateway/create_user]  route matched: POST /create_user
  09:41:02.002  +2ms   · [runtime/create_user]  bundle cache hit
  09:41:02.003  +1ms   ▶ [runtime/create_user]  executing function
  09:41:02.008  +5ms   · [db/users]              INSERT 1 row (5ms)
  09:41:02.014  +6ms   · [runtime/create_user]  queue push: send_welcome_email
  09:41:02.018  +4ms   ■ [runtime/create_user]  execution completed (15ms)

  6 spans  •  24ms total

  State changes:
    users  INSERT  id=e4a9c3f1  name="Ada"  email="ada@acme.com"
```

---

## `flux why` — Root cause in 10 seconds

```bash
flux why <request-id>
```

```
✗  POST /create_user  (240ms, 500)

ROOT CAUSE:
  error: relation "users" does not exist
  span:  db/users INSERT (line 8 of create_user/index.ts)

LIKELY ISSUE:
  Schema not pushed. The users table doesn't exist in the database.

FIX:
  Run: flux db push
```

`flux why` reads the execution record — spans, mutations, errors, timing — and
finds the root cause. No LLM required. It reads the data that's already there
and pattern-matches the most common failures: missing tables, constraint
violations, external API timeouts, N+1 patterns, permission errors.

---

## Slow span detection

Spans whose duration exceeds **500ms** are automatically flagged:

```
  09:41:02.008  +5ms   · [tool/stripe.charge]   3200ms  ⚠ SLOW
```

Configurable per command:

```bash
flux trace <id> --slow-threshold 200   # flag spans >200ms
```

---

## N+1 query detection

If the same table is queried **≥ 3 times** within a single request, Flux
flags it as a probable N+1 pattern:

```
  3 probable N+1 patterns:
    ⚠ table posts (3 queries)  consider batching with IN or preloading
```

**Before:**
```typescript
// ❌ N+1 — queries posts table once per user
const users = await ctx.db.users.findMany();
for (const user of users) {
  const posts = await ctx.db.posts.findMany({ where: { user_id: { eq: user.id } } });
}
```

**After:**
```typescript
// ✅ Single query
const users = await ctx.db.users.findMany();
const userIds = users.map(u => u.id);
const posts = await ctx.db.query(
  "SELECT * FROM posts WHERE user_id = ANY($1)", [userIds]
);
```

---

## Missing index suggestions

When a DB query scans more rows than it returns and no matching index exists,
Flux suggests one:

```
  1 missing index suggestion:
    → posts.user_id  run: CREATE INDEX ON posts(user_id);
```

---

## State inspection

### Row history

```bash
flux state history users --id e4a9c3f1
```

```
  v1  INSERT  2026-03-12 09:41:02  request a3f9d2b1  create_user
      name="Ada"  email="ada@acme.com"

  v2  UPDATE  2026-03-12 09:42:15  request 72af9c1d  update_user
      name="Ada"  → name="Ada Lovelace"

  v3  UPDATE  2026-03-12 09:45:03  request 8be14a2f  upgrade_plan
      plan="free"  → plan="pro"
```

Every version linked to the request that caused it.

### Blame

```bash
flux state blame users
```

```
  id=e4a9c3f1  last_write: request 8be14a2f  upgrade_plan  2026-03-12 09:45
  id=7f3a1b2c  last_write: request 550e8400  create_user   2026-03-12 09:41
```

---

## Incident replay

```bash
flux incident replay <request-id>
```

Re-executes the request with the original input and code version. External
calls are mocked using the recorded responses. Side effects (emails, webhooks)
are suppressed. The replay produces a new execution record that can be compared
with the original:

```bash
flux trace diff <original-id> <replay-id>
```

```
  SPAN              ORIGINAL    REPLAY     DELTA
  gateway           2ms         2ms        0ms
  create_user       1ms         1ms        0ms
  db.users SELECT   12ms        11ms       -1ms
  stripe.charge     3200ms      95ms       -3105ms  ✗→✔

  → stripe.charge regressed by 3105ms
```

---

## Live monitoring

```bash
flux tail                    # stream all requests
flux tail --errors           # only errors
flux logs create_user --follow   # tail one function
flux errors                  # per-function error summary
```

```
POST  /create_user    create_user   24ms   ✔
  users.id=e4a9c3f1  INSERT  name="Ada"

POST  /checkout       checkout      3.2s   ✗ 500
  error: Stripe timeout
  → flux why 550e8400
```

---

## Execution record retention

| Environment | Default retention | Override |
|---|---|---|
| Local (`flux dev`) | Forever (until `flux dev --clean`) | — |
| Self-hosted | 30 days | `RETENTION_DAYS` env var |
| Fluxbase cloud | 90 days (free), 1 year (pro) | Dashboard setting |

Errors are always retained at the maximum tier regardless of sample rate.

---

*For the complete observability spec, see
[framework.md §18](framework.md#18-observability--debugging).*
