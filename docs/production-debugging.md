# Production Debugging

Flux transforms production incidents into a searchable audit trail. This
document covers the complete debugging workflow built on top of
[Execution Records](concepts.md#execution-record).

---

## The problem

When a backend fails in production, a typical debugging session:

1. Open log aggregator, start `grep`-ing
2. Find a vague error message without enough context
3. Add more logging, redeploy, wait for the bug to happen again
4. Repeat

This is the state of the art. Logs show *that* something failed but not
*the execution* that caused it. There is no `git show` for a backend request.

---

## What Flux records

Every request produces three linked records, all joined by `request_id`:

| Record | What it captures |
|---|---|
| **Execution spans** | Timing for every layer: gateway, runtime, DB, external calls |
| **Database mutations** | Before/after JSONB for every INSERT, UPDATE, DELETE |
| **Request envelope** | Method, path, function name, input, output, error, code_sha |

This is append-only. Every execution is permanent. A failure from three days
ago has the same inspectability as one from five seconds ago.

---

## The debugging workflow

### 1. Watch production (`flux tail`)

```bash
flux tail
```

```
POST  /create_user    24ms   ✔
  users.id=e4a9c3f1  INSERT  name="Ada"

POST  /checkout       3.2s   ✗ 500
  error: Stripe timeout
  → flux why 550e8400
```

Live requests with inline data mutations. Errors surface immediately with
the next command to run.

### 2. Root cause (`flux why`)

```bash
flux why 550e8400
```

```
✗  POST /checkout → checkout  (3200ms, 500)

ROOT CAUSE:
  error: Stripe API timeout after 10000ms
  span:  tool/stripe.charge (line 42 of checkout/index.ts)
  code_sha: a93f42c

STATE CHANGES:
  orders  INSERT  id=7f3a  total=99.00  status="pending"

FIX SUGGESTION:
  External call timeout. Increase timeout or add retry with backoff.
```

### 3. Step through execution (`flux trace debug`)

```bash
flux trace debug 550e8400
```

```
  Step 1/4  gateway
  Input:  POST /checkout  { plan: "pro" }
  Output: routed to checkout function
  Time:   2ms

  [Enter] next   [s] state   [p] prev   [q] quit
```

Interactive step-through of every span — what the function received, what it
returned, and what state changed at each step.

### 4. Compare executions (`flux trace diff`)

```bash
flux trace diff 550e8400 9f3a1b2c
```

```
  SPAN              ORIGINAL    REPLAY     DELTA
  gateway           2ms         2ms        0ms
  checkout          1ms         1ms        0ms
  db.orders         12ms        11ms       -1ms
  stripe.charge     3200ms      95ms       -3105ms  ✗→✔

  → stripe.charge regressed by 3105ms
```

Compare any two executions: original vs replay, before vs after deploy,
user A vs user B.

### 5. Find the breaking commit (`flux bug bisect`)

```bash
flux bug bisect --function checkout --good a93f42c --bad 7e1b3d8
```

```
  Bisecting 47 commits...
  Testing a1f9c3e... ✔ pass   (avg 94ms, 0 errors)
  Testing 4d8a2b1... ✗ fail   (avg 3200ms, 12% errors)

  First bad commit: 4d8a2b1
  Author: dev@acme.com
  Change: stripe timeout 5000 → 10000
```

Binary search over recorded executions. No replays required — just reads
the execution records that already exist for each code version.

### 6. Replay an incident (`flux incident replay`)

```bash
flux incident replay 550e8400
```

```
  Replaying request 550e8400 with recorded state...
  [gateway]      ✔ 2ms
  [checkout]     ✔ 1ms
  [db.orders]    ✔ 11ms
  [stripe.charge] ⚙ mocked from recorded response

  Replay complete. New execution: 9f3a1b2c
  Compare: flux trace diff 550e8400 9f3a1b2c
```

Re-executes with the original input and `code_sha`. External calls are mocked
using recorded responses. Side effects suppressed (`x-flux-replay: true`).

### 7. Row-level audit (`flux state history`)

```bash
flux state history users --id e4a9c3f1
```

```
  v1  INSERT  2026-03-12 09:41:02  request a3f9d2b1  create_user
      name="Ada"  email="ada@acme.com"  plan="free"

  v2  UPDATE  2026-03-12 09:42:15  request 72af9c1d  confirm_email
      email_verified: false → true

  v3  UPDATE  2026-03-12 09:45:03  request 8be14a2f  upgrade_plan
      plan: "free" → "pro"
```

Every version linked to the request that caused it. `git blame` for database rows.

---

## Why only Flux can do this

Logs, metrics, and traces are necessary but insufficient:

| Signal | Records | Missing |
|---|---|---|
| Logs | Messages | Causal chain, state context |
| Metrics | Counters, percentiles | Individual request detail |
| Traces | Timing per span | What data changed |

None tell you *what state the system was in* when the failure happened.

Flux records execution + state transitions in the same transaction. That
combination is what makes deterministic replay possible. Other platforms
can't do this because they don't control the full stack:

- AWS Lambda doesn't see load balancer decisions
- Vercel doesn't control the database
- Cloudflare doesn't control the runtime

Flux controls Gateway → Runtime → Data Engine → Postgres. Every layer
captures data. That's the architectural requirement.

---

## The Git analogy

| Git | Flux | What it does |
|-----|------|-------------|
| `git show` | `flux why` | Full context of what happened |
| `git log` | `flux state history` | Version history of a record |
| `git blame` | `flux state blame` | Who last wrote this row |
| `git diff` | `flux trace diff` | Compare two executions |
| `git bisect` | `flux bug bisect` | Find the breaking commit |
| `git revert` | `flux incident replay` | Reproduce and verify |

---

*For the complete observability spec, see
[framework.md §18](framework.md#18-observability--debugging).*
