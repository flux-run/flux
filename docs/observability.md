# Observability

Fluxbase automatically instruments every request end-to-end. No code changes
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

If the same table is queried **≥ 3 times** within a single request, Fluxbase
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

When Fluxbase detects the same `(table, filter_column)` pair in **≥ 2 slow spans**
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
  -H "X-Fluxbase-Tenant: $TENANT_ID"
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
