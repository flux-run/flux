# Building Git for Backend Execution

*Why production debugging is still primitive — and what it looks like when it isn't.*

---

## The problem

When a backend fails in production, a typical debugging session looks like this:

1. An alert fires (or a user reports it)
2. You open the log aggregator and start `grep`-ing
3. You find a vague error message without enough context
4. You add more logging and redeploy
5. You wait for the bug to happen again
6. You repeat

This is the state of the art in 2026. And it's not because we lack tooling — we have more observability infrastructure than ever. We have:

- **Logs** — what happened, in plain text
- **Metrics** — how often it happened, and how fast
- **Traces** — which services were involved, and how long each took

But there's something fundamental missing. When your function runs in production, processes a Stripe webhook, touches three database rows, sends an email and crashes at line 87 of `create_user.ts` — you can see *that* it failed, but you cannot reconstruct *the execution* that caused it.

There is no equivalent of:

```bash
git show <commit>
git diff HEAD~1
git bisect run test
```

for a backend request. Every execution disappears the moment it finishes.

---

## The insight: treat execution like version control

Git changed software development by making every code change permanent and inspectable. Before Git, "what changed?" was a hard question. After Git, it's trivial.

The same shift is possible for backend execution.

**The key insight:** every request should leave behind a complete forensic record — execution spans, data mutations, and request metadata — all tied to a single `request_id`. Once you have that, production debugging becomes deterministic.

Instead of reconstructing what happened from scattered logs, you can *retrieve* what happened, directly.

---

## What Fluxbase records

Every request through Fluxbase produces three linked records:

### 1. Trace spans

Every layer of the stack emits a structured span:

| Layer       | Span type       | What it captures                      |
|-------------|-----------------|---------------------------------------|
| Gateway     | `gateway`       | auth, rate limit, routing, latency    |
| Runtime     | `function`      | entry, tool calls, errors, exit       |
| Tool calls  | `http_request`  | external API, duration, status        |
| Data engine | `db_query`      | SQL, table, row count, duration       |

### 2. State mutations

Every INSERT, UPDATE, and DELETE is recorded with full before/after snapshots:

```sql
-- state_mutations table (simplified)
request_id   text
table_name   text
operation    text              -- INSERT / UPDATE / DELETE
before_state jsonb
after_state  jsonb
version      int               -- per-row monotonic counter
span_id      text              -- links to the exact span that caused this write
```

This is append-only. Every mutation is permanent. A row's entire history is always recoverable.

### 3. Request envelope

The gateway records the full HTTP context:

```json
{
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "method":     "POST",
  "path":       "/signup",
  "function":   "create_user",
  "started_at": "2026-03-11T09:41:02.003Z",
  "duration_ms": 3210,
  "status":      500
}
```

All three records share the same `request_id`. This is the join key that makes everything work.

---

## What the execution chain looks like

A typical request through Fluxbase produces a chain that looks like this:

```
POST /signup  →  request_id: 550e8400
│
├─ span: gateway           2ms    auth ✔, rate_limit ✔
├─ span: create_user       1ms    function start
├─ span: db.users SELECT  12ms    rows: 0
├─ span: stripe.charge  3200ms   ⚠ SLOW  status: timeout
│
└─ state_mutations
     users  id=7f3a   INSERT   plan=free  email=user@acme.com
```

Every field, every mutation, every millisecond — recorded and linked.

---

## The Git-style debugging workflow

Once execution is recorded, the CLI can work like Git against that history.

### Watch production requests

```
$ flux tail

POST  /login          auth_user     38ms    ✔
   users.id=7f3a  last_login_at → 2026-03-11T09:40:52Z

POST  /signup         create_user   3.2s    ✗ 500
   error: Stripe timeout after 10000ms
   → flux why 550e8400
   users.id=7f3a  plan free → pro
```

`flux tail` shows every live request with inline data mutations. Errors surface immediately with the error message and the next command to run.

### Inspect an execution (`git show`)

```
$ flux why 550e8400

✗  POST /signup → create_user  (3200ms, 500 FAILED)
    request_id:  550e8400-e29b-41d4-a716-446655440000
    error:       Stripe timeout after 10000ms

─── Execution graph ─────────────────────────────────────
  gateway     POST /signup           2ms
  runtime     create_user            1ms
  db          users (SELECT)        12ms
  tool        stripe.charge       3200ms  ⚠ slow

─── State changes ───────────────────────────────────────
  users  v1  INSERT  id=7f3a
    email:  user@acme.com
    plan:   free

─── Previous request ────────────────────────────────────
  ✔ POST /login  38ms  (0.2s before)
  ⚠ also modified  users.id=7f3a
```

This is the answer to "what happened?" — execution graph, data changes, and a pointer to the request that ran just before it.

### Let the system diagnose the failure

```
$ flux doctor 550e8400

REQUEST
────────────────────────────────
  POST /signup
  function: create_user
  duration: 3.20s
  status:   500

ROOT CAUSE
────────────────────────────────
  ⚡ stripe.charge timed out after 10000ms

LIKELY ISSUE
────────────────────────────────
  External tool latency exceeded threshold.

EVIDENCE
────────────────────────────────
  stripe.charge     3200ms  ⚠ slow
  db.users          12ms
  runtime           1ms

DATA CHANGES
────────────────────────────────
  users.id=7f3a  insert
    plan:   free
    email:  user@acme.com

SUGGESTED ACTIONS
────────────────────────────────
  • Increase timeout above 11000ms
  • Add retry with exponential backoff for stripe.charge
  • Check network latency to the external service
  • flux why 550e8400
  • flux trace debug 550e8400
```

`flux doctor` runs a small rules engine over the spans and mutations. It doesn't need an LLM. It just reads the data that's already there. The most common failures — slow external calls, missing indexes, null access, race conditions — are pattern-matchable.

### Step through the execution (`git show --patch`)

```
$ flux trace debug 550e8400

  Step 1/4  gateway
  Input:  POST /signup  { email: "user@acme.com" }
  Output: { tenant_id: "t_abc123", passed: true }
  Time:   2ms

  [Enter] next   [s] state   [p] prev   [q] quit
```

An interactive step-through of every span. At each step you can inspect what the function received, what it returned, and what state changed.

### Compare two executions (`git diff`)

```
$ flux trace diff 550e8400 9f3a1b2c

  SPAN              ORIGINAL    REPLAY     DELTA
  gateway           2ms         2ms        0ms
  create_user       1ms         1ms        0ms
  db.users SELECT   12ms        11ms       -1ms
  stripe.charge     3200ms      95ms       -3105ms  ✗→✔

  → stripe.charge regressed by 3105ms
```

Compare any two executions — the original failure and a replay, two different users, before/after a deploy. The diff shows exactly where they diverged.

### Find the commit that broke something (`git bisect`)

```
$ flux bug bisect --function create_user --period 24h

  Bisecting 47 commits between a93f42c (last good) and 7e1b3d8 (first bad)...
  Testing a1f9c3e... ✔ pass   (avg 94ms, 0 errors)
  Testing 4d8a2b1... ✗ fail   (avg 3200ms, 12% error rate)

  First bad commit: 4d8a2b1
  Author: dev@acme.com
  diff: stripe timeout 5000 → 10000
```

### Track every write to a row (`git blame`)

```
$ flux state blame users 7f3a

  v1  INSERT  2026-03-11 09:41:02  request 550e8400  create_user
  v2  UPDATE  2026-03-11 09:41:15  request 72af9c1d  confirm_email
  v3  UPDATE  2026-03-11 09:42:03  request 8be14a2f  upgrade_plan
```

A full audit trail for any database row. Every version linked to the request that caused it.

### Replay any execution deterministically

```
$ flux incident replay 550e8400

  Replaying request 550e8400 with saved state snapshot...
  [gateway]      ✔ 2ms
  [create_user]  ✔ 1ms
  [db.SELECT]    ✔ 11ms  — users.id=7f3a found (from snapshot)
  [stripe.charge] ⚙ using mock (external calls replaced)

  Result: same DB mutations reproduced. 3 of 3 assertions passed.
```

---

## The architecture that makes this possible

```
HTTP client
     │
     ▼
┌─────────────────────────────────────────────────────┐
│  Gateway                                            │
│  auth · rate limit · span emit · request envelope  │
└─────────────────────────────────────────────────────┘
     │  request_id propagated in every span
     ▼
┌─────────────────────────────────────────────────────┐
│  Runtime  (Deno/V8 isolate per tenant)              │
│  your TypeScript function · tool calls · spans     │
└─────────────────────────────────────────────────────┘
     │  mutations intercepted before they reach Postgres
     ▼
┌─────────────────────────────────────────────────────┐
│  Data Engine                                        │
│  schema validation · policies · mutation log       │
└─────────────────────────────────────────────────────┘
     │  standard SQL
     ▼
  PostgreSQL  (you own this, Fluxbase never touches it directly)
```

The data engine sits between your function code and Postgres. It compiles the schema into a type-safe query API, enforces access policies, and records every mutation in the same transaction as the write itself. The mutation record is never optional — it's atomic with the data change.

Three properties make the recording reliable:

1. **Atomic writes** — the mutation log entry commits in the same transaction as the data change. If the write rolls back, the log entry rolls back too.

2. **Span IDs on mutations** — every mutation row carries the `span_id` that caused it. You can always trace a database change back to the exact line of function code that triggered it.

3. **Immutable history** — the `state_mutations` table is append-only. There is no UPDATE or DELETE on audit records.

---

## Why existing tools can't do this

The reason logs, metrics, and traces are insufficient isn't a tooling failure — it's a data model problem.

| Signal  | Records                | Missing                        |
|---------|------------------------|--------------------------------|
| Logs    | messages               | causal chain, state context    |
| Metrics | counters, percentiles  | individual request detail      |
| Traces  | timing per span        | what data changed              |

None of these tell you *what state the system was in* when the failure happened.

Fluxbase records execution + state transitions. That combination is what makes deterministic replay possible.

A failure that happened three days ago at 2am has the same inspectability as one that happened five seconds ago. The data is always there. You don't need to reproduce the bug — you just need to look it up.

---

## The debugging surface this creates

| Command               | Git equivalent    | What it does                                  |
|-----------------------|-------------------|-----------------------------------------------|
| `flux tail`           | `git log -f`      | Stream live requests                          |
| `flux why <id>`       | `git show`        | Full execution: spans + mutations + context   |
| `flux doctor <id>`    | —                 | Automatic diagnosis (no Git equivalent)       |
| `flux trace debug`    | `git show -p`     | Interactive step-through                      |
| `flux trace diff`     | `git diff`        | Compare two executions                        |
| `flux bug bisect`     | `git bisect`      | Find the commit that introduced a failure     |
| `flux state blame`    | `git blame`       | Who mutated this row and when                 |
| `flux incident replay`| `git stash apply` | Deterministic reproduction                    |
| `flux state history`  | `git log -- file` | Full version history for a database row       |

The last column — `flux doctor` — has no Git equivalent because Git only knows about code. Fluxbase knows about code *and* runtime state, which is why automated diagnosis is possible.

---

## What this changes in practice

Before Fluxbase, a production incident typically looked like:

1. Alert fires
2. `grep` through logs for 20 minutes
3. Can't reproduce locally (different data, different timing)
4. Add more logging, redeploy, wait
5. Spot the issue, fix it, deploy again
6. Repeat until resolved

With execution history:

1. Alert fires, error includes `request_id`
2. `flux doctor <id>` — see diagnosis in seconds
3. `flux trace debug <id>` — step through what actually ran
4. Fix the issue
5. `flux trace diff <failed> <fixed>` — verify the change fixed it

The difference isn't speed. It's *certainty*. You're no longer guessing at what happened — you're reading what happened.

---

## The key insight, stated plainly

Git made code history permanent. Every change is recorded, every commit is inspectable, every regression is bisectable.

Fluxbase applies the same model to backend execution. Every request is recorded. Every mutation is inspectable. Every regression is bisectable.

The biggest problem in production debugging isn't lack of logging — it's that execution history disappears the moment a request finishes.

Fluxbase keeps that history.

---

*Fluxbase is in early access. The quickstart takes five minutes and works with any TypeScript function and a Postgres database you already have.*

*[Start here →](https://fluxbase.co/docs/quickstart)*
