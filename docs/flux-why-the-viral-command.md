# The Viral Command: `flux why`

## One command that changes debugging

```bash
$ flux why 550e8400-e29b-41d4-a716-446655440000

✗  POST /checkout → checkout  (3200ms, 500)

ROOT CAUSE:
  error: Stripe API timeout after 10000ms
  span:  tool/stripe.charge (line 42 of checkout/index.ts)
  code_sha: a93f42c

STATE AT FAILURE:
  orders  INSERT  id=7f3a  total=99.00  status="pending"

FIX SUGGESTION:
  External call timeout. Increase timeout or add retry with backoff.
  Test with: flux incident replay 550e8400
```

---

## Why this command is viral

### Before

```
Error in production. 2 hours:
  Check logs → spin up local env → try to recreate → git blame → deploy fix → hope
```

### After

```
Error in production. 10 seconds:
  $ flux why <request-id>
  Root cause + code + state changes + fix.
```

`flux why` is not just a debugger — it's a time machine. It says: "Here's what
happened, when, why, and what changed to cause it." It works on real production
traffic, shows exact code, finds root cause (not symptoms), and suggests a fix.

---

## How it works

`flux why` reads the [Execution Record](concepts.md#execution-record) and
runs a rules engine over the spans, mutations, and errors:

```
Input: request_id
  │
  ├── 1. Fetch trace     (execution_records + execution_spans)
  ├── 2. Fetch mutations  (execution_mutations — before/after diffs)
  ├── 3. Extract code_sha (which deploy was running)
  ├── 4. Find error span  (which span failed, and why)
  ├── 5. Pattern match    (timeout? constraint violation? N+1? missing table?)
  └── 6. Suggest fix      (based on error pattern)
  │
  ▼
Output: ROOT CAUSE + STATE + FIX
```

No LLM required. The most common failures — external timeouts, constraint
violations, missing tables, permission errors, N+1 patterns — are
pattern-matchable from the execution record data.

---

## What makes it possible

`flux why` requires the full execution chain to be recorded:

| Component | What it provides |
|---|---|
| Gateway | Request envelope (method, path, headers) + trace root |
| Runtime | Execution spans (timing, errors, tool calls) + `code_sha` |
| Data Engine | Database mutations (before/after for every write) |
| Postgres | Durable storage for all records |

This is why `flux why` only works on Flux — it requires control over the
entire stack from request to database.

| Platform | Can trace? | Can see mutations? | Can bisect? | Can replay? |
|---|---|---|---|---|
| AWS Lambda | Partial | No | No | No |
| Vercel | Partial | No | No | No |
| Cloudflare Workers | Partial | No | No | No |
| Temporal | Partial | Partial | No | Partial |
| **Flux** | **Yes** | **Yes** | **Yes** | **Yes** |

---

## The 10-second promise

1. An error occurs in production
2. The response includes `x-request-id`
3. Run `flux why <request-id>`
4. See: root cause, state changes, code location, fix suggestion
5. Optionally replay: `flux incident replay <request-id>`
6. Compare: `flux trace diff <original> <replay>`

From alert to root cause in under 10 seconds. No log diving, no local
reproduction, no guesswork.

---

*For the full observability spec, see
[framework.md §18](framework.md#18-observability--debugging).
For the debugging workflow, see [production-debugging.md](production-debugging.md).*
