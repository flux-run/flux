# Observability

Flux automatically instruments every request end-to-end. No code changes
required — just run `flux trace <request-id>`.

---

## Distributed tracing

Every request routed through the gateway receives a unique `x-request-id`
(UUID v4).  Spans are written to the `platform_logs` table as the request
flows through each service:

| Source | What is instrumented |
|---|---|
| `gateway` | Route matched, cache hit/miss |
| `runtime` | Bundle cache, R2 fetch, execution start/end |
| `function` | `ctx.log()` calls |
| `db` | Every `/db/query` — table, duration, cache, filter columns |
| `hook` | Row-level hooks triggered by mutations |
| `event` | Events emitted by functions (`ctx.event.emit`) |
| `workflow` | Workflow step start/end |
| `cron` | Schedule trigger execution |

### Reading a trace

```bash
# Get the request ID from the response header
curl -D - https://YOUR_GATEWAY/greet -d '...' 2>&1 | grep x-request-id

# Display the full trace
flux trace a3f9d2b1-4c8e-4f7d-b2e1-9d0c3a5f8e2b
```

Sample output:

```
Trace a3f9d2b1-...  284ms end-to-end
  ⚠ 1 slow span (>500ms)

  12:00:01.000  +0ms     ▶ [gateway/list_posts]  INFO   route matched: POST /list_posts
  12:00:01.002  +2ms     · [runtime/list_posts]  DEBUG  bundle cache hit
  12:00:01.006  +4ms     ▶ [runtime/list_posts]  INFO   executing function
  12:00:01.010  +4ms     · [db/posts]            INFO   db query on posts (8ms)
  12:00:01.010  +0ms     · [db/posts]            INFO   db query on posts (6ms) ⚠ N+1
  12:00:01.011  +1ms     · [db/posts]            WARN   slow db query on posts (72ms) ⚠ N+1
  12:00:01.084  +73ms    ■ [runtime/list_posts]  INFO   execution completed (78ms)

  7 spans  •  284ms total

  3 probable N+1 patterns:
    ⚠ table posts (3 queries)  consider batching with IN or preloading all at once

  1 slow db query (>50ms) — check indexes on the flagged tables

  1 missing index suggestion:
    → posts.user_id  run: CREATE INDEX ON posts(user_id);
```

---

## Slow span detection

Spans whose duration exceeds **500 ms** are automatically flagged as slow in
the trace summary.  The threshold is configurable per call:

```bash
flux trace <id> --slow-threshold 200   # flag spans >200ms
```

---

## N+1 query detection

If the same table is queried **≥ 3 times** within a single request, Flux
flags it as a probable N+1 pattern.  Affected spans are tagged with `⚠ N+1`
in the trace view, and the summary lists each table with a fix hint.

**Example N+1 scenario:**

```javascript
// ❌ N+1 — queries posts table once per user
const users = await getUsers();
for (const user of users) {
  const posts = await getPosts(user.id);   // 1 query per user
}
```

**Fix:**

```javascript
// ✅ Single query — fetch all posts at once
const users = await getUsers();
const userIds = users.map(u => u.id);
const posts = await getPostsByUserIds(userIds);   // 1 query total
const postsByUser = groupBy(posts, p => p.user_id);
```

---

## Slow query detection

Individual DB queries taking **> 50 ms** are emitted at `WARN` level in the
trace.  The span includes:

```json
{
  "source": "db",
  "level": "warn",
  "message": "slow db query on orders (72ms)",
  "metadata": {
    "table": "orders",
    "duration_ms": 72,
    "cache": "miss",
    "slow": true,
    "filter_cols": ["user_id", "status"]
  }
}
```

---

## Automatic index suggestions

When Flux detects the same `(table, filter_column)` pair in **≥ 2 slow spans**
within a single request, it automatically emits a `CREATE INDEX` suggestion in
the trace envelope:

```json
{
  "suggested_indexes": [
    {
      "table":  "orders",
      "column": "user_id",
      "ddl":    "CREATE INDEX ON orders(user_id);"
    }
  ]
}
```

The CLI renders this at the bottom of the trace output:

```
1 missing index suggestion:
  → orders.user_id  run: CREATE INDEX ON orders(user_id);
```

You can copy-paste the DDL and run it against your project's database.

This detection (slow + repeated filter on the same column = missing index) is:

- Zero configuration — works out of the box
- Non-intrusive — no query plan analysis, no EXPLAIN calls
- Actionable — produces ready-to-run DDL

---

## Flame graph

For a visual waterfall of span durations:

```bash
flux trace <id> --flame
```

```
  Flame graph

  gateway/list_...  ████░░░░░░░░░░░░░░░░░░░░░░░░░░  +0ms      10ms
  runtime/list_...  ░░░░████████████████████████████  +10ms    274ms
  db/posts         ░░░░░░██░░░░░░░░░░░░░░░░░░░░░░░░  +12ms      8ms  (3×)
```

---

## Raw trace data

Traces are accessible via the API for integrations:

```bash
curl https://api.fluxbase.co/logs/trace/<request-id> \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Flux-Tenant: $TENANT_ID"
```

Response envelope fields:

| Field | Description |
|---|---|
| `spans` | Array of span objects with metadata |
| `total_duration_ms` | End-to-end wall time |
| `slow_span_count` | Spans exceeding the slow threshold |
| `n_plus_one_tables` | Tables flagged for N+1 patterns |
| `slow_db_count` | DB spans flagged as slow |
| `suggested_indexes` | Auto-generated `CREATE INDEX` DDL |

---

## State mutation log

Every write through the Flux data engine is recorded in `flux_internal.state_mutations`. This is the foundation for `flux why`, `flux state history`, `flux state blame`, and `flux incident replay`.

### Schema

```sql
CREATE TABLE flux_internal.state_mutations (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NOT NULL,
    project_id  uuid NOT NULL,
    request_id  text,          -- links to platform_logs / trace
    table_name  text NOT NULL,
    record_pk   jsonb NOT NULL, -- e.g. {"id": 42}
    operation   text NOT NULL,  -- insert | update | delete
    before_state jsonb,
    after_state  jsonb,
    actor_id    text,           -- api-key slug, user ID, or system
    version     bigint NOT NULL DEFAULT 1,
    created_at  timestamptz NOT NULL DEFAULT now()
);
```

Rows are immutable: every write appends a new version. `version` is incremented per-row atomically in a transaction.

### Data engine endpoints

| Endpoint | Purpose |
|---|---|
| `GET /db/mutations?request_id=&limit=` | All mutations caused by one request (used by `flux why`) |
| `GET /db/history/:database/:table?id=&limit=` | Version history for a single row (used by `flux state history`) |
| `GET /db/blame/:database/:table?limit=` | Last writer per row (used by `flux state blame`) |
| `GET /db/replay/:database?from=&to=&limit=` | All mutations in a time window (used by `flux incident replay`) |

### Using the API directly

```bash
# All state mutations caused by one request
curl https://api.fluxbase.co/db/mutations?request_id=9624a58d \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Flux-Tenant: $TENANT_ID" \
  -H "X-Flux-Project: $PROJECT_ID"

# Response
{
  "request_id": "9624a58d...",
  "count": 2,
  "mutations": [
    {
      "table_name":   "users",
      "record_pk":    {"id": 42},
      "operation":    "insert",
      "before_state": null,
      "after_state":  {"id": 42, "email": "ada@example.com", "plan": "free"},
      "actor_id":     "api-key-prod",
      "version":      1,
      "created_at":   "2026-03-11T14:01:12Z"
    },
    {
      "table_name":   "users",
      "record_pk":    {"id": 42},
      "operation":    "update",
      "before_state": {"plan": "free"},
      "after_state":  {"plan": "pro"},
      "actor_id":     "api-key-prod",
      "version":      2,
      "created_at":   "2026-03-11T14:01:12Z"
    }
  ]
}
```

---

## x-request-id propagation

`x-request-id` is generated at the gateway edge and propagated through every service boundary in the call chain. All platform components write it into every span and log line they produce.

### Propagation chain

```
Client
  ↓  (response header: x-request-id)
Gateway
  ↓  x-request-id, x-parent-span-id  (forwarded to Runtime + Data Engine)
Runtime
  ↓  x-request-id  (forwarded to Control Plane for log ingestion)
Hooks     ← x-request-id passed as parent context when hooks fire
Events    ← x-request-id stored alongside every emitted event
Workflows ← x-request-id propagated into each workflow step execution
Cron jobs ← x-request-id generated at trigger time, carried through execution
```

This means a single `flux trace <id>` or `flux why <id>` call can recover the full causal chain — gateway → function → db → hooks → downstream events — for any request.

### Headers forwarded by the gateway

| Header | Type | Description |
|---|---|---|
| `x-request-id` | UUID v4 | Unique per request; client may supply, gateway generates if absent |
| `x-parent-span-id` | UUID v4 | Current gateway span ID; becomes the parent for runtime spans |
| `X-Tenant-Id` | UUID | Tenant identifier |
| `X-Tenant-Slug` | string | Human-readable tenant slug |

---

## Replay mode (`x-flux-replay: true`)

When the `x-flux-replay: true` header is present on a request, Flux suppresses all side effects while still applying state mutations. This enables deterministic replay of past incidents without re-triggering emails, webhooks, or external API calls.

### What is suppressed in replay mode

| Component | Replay behaviour |
|---|---|
| Row-level hooks | Not fired |
| Event emission (`ctx.event.emit`) | Silently dropped |
| Workflow triggers | Not started |
| Cron-triggered executions | Not re-scheduled |
| State mutations (`/db/query`) | **Applied** — this is the point of replay |

### How replay requests are identified

The `x-request-id` for replay requests uses the format `replay:<original-request-id>` so replays are distinguishable in traces and logs from the original runs.

### Invoking replay directly

```bash
# Replay a single request (side effects suppressed)
curl -X POST https://api.fluxbase.co/db/query \
  -H "x-flux-replay: true" \
  -H "x-request-id: replay:9624a58d-57e7-..." \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Flux-Tenant: $TENANT_ID" \
  -H "X-Flux-Project: $PROJECT_ID" \
  -d '{"database": "default", "table": "users", "operation": "update", ...}'
```

The CLI commands `flux incident replay` and `flux trace <id> --replay` handle constructing the correct mutation payloads and headers automatically.
