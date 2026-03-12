# Git for Backend Execution

*Why production debugging is still primitive — and what it looks like when it isn't.*

---

## The insight

Git changed software development by making every code change permanent and
inspectable. Before Git, "what changed?" was a hard question. After Git,
it's trivial.

The same shift is possible for backend execution.

**Every request should leave behind a complete forensic record** — execution
spans, data mutations, and request metadata — all tied to a single
`request_id`. Once you have that, production debugging becomes deterministic.

---

## Before Flux

```
Error in production. Spend 2 hours:
1. Check logs (thousands of lines)
2. Spin up local environment (30 min setup)
3. Try to recreate issue (can't without production data)
4. Dig through git blame (who changed what)
5. Deploy fix (wait for deploy)
6. Hope it works
```

## After Flux

```
Error in production. 10 seconds:
$ flux why <request-id>

Root cause + code_sha + state changes + fix suggestion.
Test with: $ flux incident replay <request-id>
Deploy with confidence.
```

---

## What makes it work

Three records, one join key:

### 1. Execution spans

Every layer of the stack emits timing:

| Layer | What it captures |
|---|---|
| Gateway | Auth, rate limit, routing, latency |
| Runtime | Bundle fetch, execution, tool calls, errors |
| Data Engine | SQL query, table, row count, duration |
| Queue | Enqueue, wait time, worker execution |

### 2. Database mutations

Every INSERT, UPDATE, DELETE captured with before/after state:

```typescript
interface DbMutation {
  table:     string;
  operation: "INSERT" | "UPDATE" | "DELETE";
  row_id:    string;
  before:    JsonValue | null;   // null for INSERT
  after:     JsonValue | null;   // null for DELETE
}
```

Append-only. Every mutation is permanent.

### 3. Request envelope

Full HTTP context: method, path, function, input, output, error, timing,
`code_sha` (which git commit was deployed).

All three share the same `request_id`. This is the join key that makes
everything work.

---

## The debugging surface

| Git command | Flux command | What it does |
|---|---|---|
| `git show` | `flux why <id>` | Full execution context + root cause |
| `git log` | `flux state history <table>` | Version history of a database row |
| `git blame` | `flux state blame <table>` | Last writer per row |
| `git diff` | `flux trace diff <a> <b>` | Compare two executions |
| `git bisect` | `flux bug bisect` | Find the commit that broke it |
| `git revert` | `flux incident replay <id>` | Reproduce with mocked side effects |
| `git log -f` | `flux tail` | Stream live requests |

All commands operate on real production data. They are read-only except
`flux incident replay`, which re-runs with side effects suppressed.

---

## The architecture that makes it possible

```
HTTP client
     │
     ▼
Gateway    — auth, rate limit, trace root, request envelope
     │  request_id propagated in every span
     ▼
Runtime    — Deno V8 isolate, ctx object, tool calls
     │  mutations intercepted before reaching Postgres
     ▼
Data Engine — schema validation, policies, mutation log
     │  atomic: mutation + log entry in same transaction
     ▼
PostgreSQL — you own it, Flux never touches it directly
```

The Data Engine sits between function code and Postgres. It records every
mutation in the same transaction as the write itself. Three properties make
this reliable:

1. **Atomic writes** — the mutation log commits in the same transaction as
   the data change. Rollback the write → rollback the log.

2. **Span IDs on mutations** — every mutation carries the `span_id` that
   caused it. Trace any database change back to the exact function code.

3. **Immutable history** — `execution_mutations` is append-only. No UPDATE
   or DELETE on audit records.

---

## Why existing tools can't do this

| Signal | Records | Missing |
|---|---|---|
| Logs | Messages | Causal chain, state context |
| Metrics | Counters, percentiles | Individual request detail |
| Traces | Timing per span | What data changed |

None tell you *what state the system was in* when the failure happened.
Flux records execution + state transitions. That combination is what makes
replay, bisect, and blame work.

---

## The positioning

> Flux is Git for backend execution.
>
> Git made every code change inspectable, diffable, and revertable.
> Flux makes every backend execution inspectable, diffable, and replayable.
> Fluxbase is GitHub for it.
