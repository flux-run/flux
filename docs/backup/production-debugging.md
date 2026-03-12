# Production Debugging: Deterministic Replay & Retrospective Bisect

**Fluxbase transforms production incidents into a searchable audit trail with automatic regression detection and deterministic replay.**

This document covers the complete production debugging workflow:

- **Incident Replay** — Reproduce 5-minute production incidents in isolated sandbox
- **Trace Diff** — Compare executions to identify behavioral changes
- **Code Provenance** — Link execution to exact git commit (git blame)
- **Production Bisect** — Automatically find the commit that introduced a regression
- **Regression Guard** — Block deployments if real traffic shows regressions

Table of Contents:
1. [Trace Signatures: Behavioral Fingerprints](#trace-signatures-behavioral-fingerprints)
2. [What a Signature Is](#what-a-signature-is)
3. [Production Bisect](#production-bisect)
4. [Regression Detection](#regression-detection)
5. [Guard Deploy: CI for Real Traffic](#guard-deploy-ci-for-real-traffic)
6. [Why This Is Unique](#why-this-is-unique)

---

## Trace Signatures: Behavioral Fingerprints

### The Missing Piece

Fluxbase already captures:

| Component | Captures | Example |
|-----------|----------|---------|
| trace_requests | Request envelope | Method, path, headers, body, tenant, project |
| platform_logs | Execution trace | Spans, latencies, errors, tool calls |
| state_mutations | State changes | DB inserts, updates, deletes |
| code_sha | Deployed version | Git commit hash |

What was missing: **Behavior fingerprint** — a way to evaluate whether two executions behave the same.

### The Table

```sql
CREATE TABLE trace_signatures (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  
  -- Link to original request
  request_id UUID NOT NULL,
  
  -- Context
  tenant_id UUID NOT NULL,
  project_id UUID NOT NULL,
  
  -- Code version
  function_id UUID NOT NULL,
  code_sha TEXT NOT NULL,
  
  -- Behavioral signature (deterministic hash)
  signature_hash TEXT NOT NULL,  -- sha256 of behavior
  
  -- Observable outcomes
  latency_ms INTEGER NOT NULL,
  status_code INTEGER,
  error_type TEXT,               -- null = success
  
  -- When collected
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  
  -- Indexes for bisect & regression queries
  INDEX idx_function_created (function_id, created_at),
  INDEX idx_code_sha (code_sha, function_id),
  INDEX idx_request (request_id)
);
```

### When Signatures Are Created

**After every sampled request:**

```
Request: POST /checkout
↓
Gateway processes (status 201, latency 145ms)
↓
Runtime executes function
↓
Span tree completed, state mutations recorded
↓
Signature computed & written to trace_signatures
```

**During replay:**

```
Original request replayed with new code version
↓
New trace generated
↓
New signature computed & linked to original (replay_of)
↓
Can compare signature_hash(original) vs signature_hash(replay)
```

---

## What a Signature Is

A signature is a deterministic hash of the **observable behavior** of a request execution.

### Signature Inputs

**Computed from:**

```
function_id                    # Which function ran
HTTP status code               # Success (201) or failure (500)
Tool calls                      # Tool names + inputs
DB queries                      # INSERT/UPDATE/DELETE on tables
Result schema                   # Structure of response
Error code                      # If failed, what error
```

### Example: User Signup

**Execution:**

```
create_user({email: "alice@example.com", name: "Alice"})
  ├─ db.insert(users)          # status 201
  ├─ tool.gmail.send_welcome   # email sent
  └─ tool.stripe.create_customer # success
```

**Signature representation:**

```
create_user
status=201
tools=[gmail.send, stripe.create_customer]
db_ops=[users.insert]
result_schema={id, email, name, created_at}
```

**Hash:**

```
signature_hash = sha256(
  "create_user|201|gmail.send|stripe.create_customer|users.insert|{id,email,name,created_at}"
)
= "3c7d1a8f9e2b5c4d6a8f9e2b5c4d6a8f"
```

### Example: Production Regression

Original request (V1 code):

```
signature = "3c7d1a8f9e2b5c4d6a8f9e2b5c4d6a8f"
status = 201
latency = 145ms
```

Same request replayed (V2 code):

```
signature = "9bd2e1f4c5a8b2e7d9f1c3a5b7e9f1c3"  ← DIFFERENT
status = 500
latency = 2500ms
error_type = "db_timeout"
```

Signature mismatch → **regression detected automatically**.

---

## Production Bisect

### The Problem

Traditional git bisect workflow:

```
Bug detected in production
→ Developer checks out old commits locally
→ Builds, tests, marks good/bad manually
→ Slow (requires local rebuild + testing)
→ Uses synthetic tests, not real production data
```

### The Solution: Automatic Bisect

Fluxbase automates the classification using signature comparison.

Instead of human judgment:

```
Original request signature (known good)
→ Replay with each commit version in binary search
→ Compare signature_hash(replay) to signature_hash(original)
→ If same → commit is good
→ If different → commit is bad
→ Log₂(N) replays to find exact breaking commit
```

### Bisect Algorithm

**Setup:**

```
Working version: commit a82d91a (signature matches production)
Broken version: commit a93f42c (signature differs)

Commits between:
a82d91a → a83a02b → a84b13c → a85c24d → a86d35e → a87e46f → a88f57g → a89g68h → a93f42c

Total commits: 9
Binary search iterations needed: ⌈log₂(9)⌉ = 4
```

**Iteration 1: Test midpoint**

```
Test: a87e46f (midpoint)
Replay production request with a87e46f code
Compute signature

signature_hash(a87e46f) == signature_hash(a82d91a)?
→ NO
→ Bug is between a87e46f and a93f42c
→ Search right half
```

**Iteration 2:**

```
New range: a87e46f → a93f42c
Test: a8af57h (midpoint)

signature_hash(a8af57h) == signature_hash(a82d91a)?
→ YES
→ Bug is between a8af57h and a93f42c
→ Search right half
```

**Iteration 3:**

```
New range: a8af57h → a93f42c
Test: a89g68h (midpoint)

signature_hash(a89g68h) == signature_hash(a82d91a)?
→ NO
→ Bug is between a89g68h and a93f42c
→ Search right half
```

**Iteration 4:**

```
New range: a89g68h → a93f42c
Test: a91g89i (midpoint)

signature_hash(a91g89i) == signature_hash(a82d91a)?
→ NO
→ Bug is between a91g89i and a93f42c
→ Next commit is a92h90j (only one left)
→ Test a92h90j: signature differs
→ Found it!
```

**Result:**

```bash
$ flux bug bisect --trace 550e8400 --good a82d91a --bad a93f42c

Replaying production request...
Testing a87e46f: BROKEN (search right)
Testing a8af57h: OK (search right)
Testing a89g68h: BROKEN (search right)
Testing a91g89i: BROKEN (search right)

First bad commit: a92h90j

Author: dev
Date: 2026-03-08T14:23:00Z
Message: "optimize email validation regex"

Diff:
  - const EMAIL_REGEX = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
  + const EMAIL_REGEX = /^[a-zA-Z]+@[a-zA-Z]+\.[a-zA-Z]{2,}$/

Change impact: rejects emails with numbers and special chars
```

### CLI Command

```bash
flux bug bisect \
  --trace <request_id> \
  --good <commit_sha> \
  --bad <commit_sha> \
  [--mode deterministic|accelerated]
```

**Example:**

```bash
flux bug bisect \
  --trace 550e8400-e29b-41d4-a716-446655440000 \
  --good a82d91a \
  --bad a93f42c

# Output:
# Bisecting 9 commits...
# Iteration 1/4: a87e46f → BROKEN
# Iteration 2/4: a8af57h → OK
# Iteration 3/4: a89g68h → BROKEN
# Iteration 4/4: a91g89i → BROKEN
# 
# First bad commit: a92h90j
# Author: dev
# Message: optimize email validation regex
# Diff: ...
```

### Why This is Better Than Git Bisect

| Git Bisect | Fluxbase Bisect |
|-----------|-----------------|
| Manual testing | Automatic replay |
| Local environment | Production request |
| Synthetic test cases | Real user data |
| Human "good/bad" judgment | Automatic signature comparison |
| Requires checkout/build/deploy per test | Instant production replay |
| Error-prone | Deterministic |
| Slow (hours) | Fast (minutes) |

---

## Regression Detection

### Automatic Baseline Collection

As requests flow through production, signatures are collected:

```sql
SELECT code_sha, 
       avg(latency_ms) as avg_latency,
       max(latency_ms) as p99_latency,
       count(*) FILTER (WHERE error_type IS NOT NULL) as error_count
FROM trace_signatures
WHERE created_at > now() - interval '1 hour'
GROUP BY code_sha
ORDER BY created_at DESC;
```

**Example output:**

```
code_sha    avg_latency  p99_latency  error_count
a82d91a     145ms        320ms        0
a83a02b     148ms        325ms        0
a84b13c     147ms        318ms        1
a85c24d     156ms        340ms        0
a86d35e     2100ms       4500ms       127   ← REGRESSION
a87e46f     145ms        315ms        0
```

### Regression Classification

**Latency spike:**

```
avg_latency_new > avg_latency_old * 1.5
→ REGRESSION: Performance degradation
```

**Error rate increase:**

```
error_count_new > error_count_old * 1.1
→ REGRESSION: Increased failures
```

**Behavior change:**

```
signature_hash_new != signature_hash_old
→ REGRESSION: Behavioral change
```

### Alert & Automatic Rollback

```
Regression detected in a86d35e
  Latency: 145ms → 2100ms (1450% increase)
  Errors: 0 → 127

Initiating automatic rollback to a85c24d...
Traffic: 100% → 0% for a86d35e
         0% → 100% for a85c24d

Monitoring new signature distribution...
Average latency: 2100ms → 145ms ✓
Error count: 127 → 0 ✓

Rollback successful.
```

---

## Guard Deploy: CI for Real Traffic

### The Problem

Traditional CI/CD:

```
Developer pushes code
→ Tests run (synthetic scenarios)
→ Tests pass → Deploy to production
→ Real users hit regression
→ Incident
```

Tests don't catch regressions because they're not based on real traffic.

### Fluxbase Guard Deploy

```bash
flux deploy --guard
```

**What it does:**

```
1. Build new version
2. Deploy to sandbox
3. Replay 100 recent production requests to sandbox version
4. Compute signature_hash for each
5. Compare to original signatures
6. If regressions found → block deploy, show details
7. If clean → proceed with production deploy
```

### Example: Blocking a Regression

```bash
$ flux deploy --guard

Building version a86d35e...
Deploying to sandbox...

Running regression guard...
Replaying 100 production requests...

Comparing signatures:
  Request 1: ✓ (same)
  Request 2: ✓ (same)
  ...
  Request 18: ✗ REGRESSION
    Original: status 201, latency 145ms
    New:      status 500, latency 2500ms
             error: db_timeout

Blocking deploy.

Recommendations:
  1. Review changes to database layer
  2. Check query performance with explain() + analyze
  3. Increase connection pool size?
  4. Add caching layer for hot queries?
```

### Example: Passing Guard

```bash
$ flux deploy --guard

Building version a85c24d...
Deploying to sandbox...

Running regression guard...
Replaying 100 production requests...

Comparing signatures:
  Request 1: ✓ (same)
  Request 2: ✓ (same)
  ...
  Request 100: ✓ (same)

All signatures match!

Regression guard: PASSED ✓

Deploying to production...
```

### CLI Options

```bash
flux deploy --guard [--sample-count 100] [--baseline <commit_sha>]
```

- `--sample-count`: How many production requests to replay (default: 100)
- `--baseline`: Compare against specific commit (default: current production)

---

## Why This Is Unique

### The Complete Stack

| Feature | Fluxbase | AWS Lambda | Vercel | Cloudflare |
|---------|----------|-----------|--------|-----------|
| Request envelope | ✓ trace_requests | ✗ | ✗ | ✗ |
| Execution trace | ✓ platform_logs | ✗ | ✗ | ✗ |
| Code version | ✓ code_sha | ✗ | ✗ | ✗ |
| State mutations | ✓ state_mutations | ✗ | ✗ | ✗ |
| Behavior fingerprint | ✓ trace_signatures | ✗ | ✗ | ✗ |
| Incident replay | ✓ | ✗ | ✗ | ✗ |
| Production bisect | ✓ | ✗ | ✗ | ✗ |
| Regression guard | ✓ | ✗ | ✗ | ✗ |

### Automatic vs Manual

| Workflow | Manual Process | Fluxbase Automatic |
|----------|----------------|-------------------|
| Find regression | Check logs, check metrics, correlate | `flux trace blame` link to commit |
| Bisect commits | Checkout, build, test locally | `flux bug bisect --good --bad` |
| Test fix | Deploy to staging, run tests | `flux guard deploy` on real traffic |
| Compare versions | Manual trace inspection | `flux trace diff` with signature compare |

### Real Data, Not Synthetic

Fluxbase regression detection is based on:

- **Real production requests** (not synthetic test cases)
- **Real user data** (not mocks)
- **Real latencies** (not benchmarks)
- **Real errors** (not mocked failures)

This catches regressions synthetic tests would miss:

```
Example: Email regex change that breaks international domains
Synthetic test: alice+bob@example.com ✓ (passes)
Real traffic: josé@españa.es ✗ (fails)

Fluxbase guard shows: "15 requests failed after regex change"
→ Blocks deploy before users see errors
```

---

## Future Extensions

### One More Column: Time-Travel

The ultimate debugging feature requires adding just one column to `trace_signatures`:

```sql
ALTER TABLE trace_signatures ADD COLUMN
  execution_timeline JSONB;  -- Snapshots at checkpoint intervals
```

This enables:

```bash
flux state at --trace 550e8400 --checkpoint 3
```

Return the exact backend state at execution checkpoint 3 (where the error occurred), enabling complete production time-travel.

### Other Potential Features

- **Multi-function bisect**: Find which function in a workflow introduced the regression
- **Canary validation**: Automatically classify canary vs baseline requests
- **Performance budgets**: Block deploys when p99 latency increases > threshold
- **Anomaly detection**: Flag unusual execution patterns before they become incidents

---

## Complete Production Debugging Workflow

```
1. Incident detected (e.g., checkout failures 14:00-14:05)

2. Reproduce in sandbox
   $ flux incident replay 2026-03-09T14:00..14:05
   → Sandbox starts, incident reproduces

3. Identify suspect commits
   $ flux trace blame --trace 550e8400
   → Shows commits that touched checkout logic

4. Test potential fix
   $ git checkout fix/stripe-timeout
   $ cargo build -p runtime --release
   $ flux deploy --sandbox incident-replay-93821
   $ flux incident replay 2026-03-09T14:00..14:05

5. Compare results
   $ flux trace diff --original original_trace --replay patched_trace
   → errors: 127 → 0 ✓
   → latency: 2500ms → 850ms ✓

6. Find exact breaking commit (if not already known)
   $ flux bug bisect --trace 550e8400 --good a82d91a --bad a93f42c
   → First bad commit: a92h90j (email regex change)

7. Deploy with regression guard
   $ flux deploy --guard
   → Replays 100 production requests
   → All signatures match
   → Deploys to production

8. Monitor
   $ flux state inspect --at 2026-03-09T15:00
   → Verify backend state stabilized
```

---

## Execution Timeline: Time-Travel Within a Request

### The One Missing Column

To transform incident replay into **production time-travel**, add one column to `trace_signatures`:

```sql
ALTER TABLE trace_signatures ADD COLUMN
  execution_timeline JSONB;  -- Checkpoint snapshots during execution
```

### What Goes In execution_timeline

Snapshots captured at each logical checkpoint during execution:

```json
{
  "checkpoints": [
    {
      "span": "gateway.route",
      "timestamp": 1678354945000,
      "locals": {}
    },
    {
      "span": "create_user.start",
      "timestamp": 1678354945005,
      "locals": {
        "email": "alice@example.com",
        "name": "Alice"
      }
    },
    {
      "span": "db.insert",
      "timestamp": 1678354945012,
      "locals": {
        "query": "INSERT INTO users (email, name) VALUES ($1, $2)",
        "params": ["alice@example.com", "Alice"]
      },
      "state_delta": {
        "users": {
          "u123": {
            "id": "u123",
            "email": "alice@example.com",
            "name": "Alice",
            "created_at": "2026-03-09T14:02:25Z"
          }
        }
      }
    },
    {
      "span": "tool.gmail.send",
      "timestamp": 1678354945028,
      "locals": {
        "template": "welcome.html",
        "recipient": "alice@example.com",
        "subject": "Welcome to Fluxbase"
      }
    },
    {
      "span": "create_user.end",
      "timestamp": 1678354945145,
      "locals": {
        "result": {
          "id": "u123",
          "email": "alice@example.com",
          "name": "Alice"
        }
      }
    }
  ]
}
```

### Reconstruct State at Any Checkpoint

```bash
flux state at --trace 550e8400 --checkpoint 3
```

**Output:**

```
STATE AT CHECKPOINT 3
(db.insert → users table inserted)

Backend State:
  users:
    u123:
      id: u123
      email: alice@example.com
      name: Alice
      created_at: 2026-03-09T14:02:25Z

Execution Context:
  Current span: tool.gmail.send
  Locals:
    template = "welcome.html"
    recipient = "alice@example.com"
```

This enables reconstructing the exact system state at any point during a request's execution.

---

## Step-Through Production Debugger

### The Interactive Replay Session

Combine `execution_timeline` + `platform_logs` + `execution_state` to enable:

```bash
flux trace debug 550e8400
```

**Interactive session example:**

```
$ flux trace debug 550e8400

Request: POST /api/create_user
Status: 201 ✓
Duration: 145ms

Breakpoints: 5 (function_entry, db_call, tool_call, tool_result, function_exit)

[1/5] Breakpoint: create_user (entry)
    Locals:
      email = "alice@example.com"
      name = "Alice"

    > next

[2/5] Breakpoint: db.insert
    SQL:
      INSERT INTO users (email, name) VALUES ($1, $2)
    Params:
      $1 = "alice@example.com"
      $2 = "Alice"

    > next

[3/5] Breakpoint: tool.gmail.send
    Call:
      composio.gmail.send_email({
        template: "welcome.html",
        recipient: "alice@example.com",
        subject: "Welcome to Fluxbase"
      })

    > next

[4/5] Breakpoint: tool.gmail.send (result)
    Result:
      status: "queued"
      message_id: "CADc-_xabc123"

    > next

[5/5] Breakpoint: create_user (exit)
    Return value:
      {
        id: "u123",
        email: "alice@example.com",
        name: "Alice"
      }

    Session complete ✓
```

### Step Commands

| Command | Effect |
|---------|--------|
| `next` (or `n`) | Step to next checkpoint |
| `step-in` (or `s`) | Step into tool call details |
| `continue` (or `c`) | Continue to next error (if any) |
| `locals` (or `l`) | Show local variables at current checkpoint |
| `state` | Show backend state mutations so far |
| `exit` (or `q`) | End session |

### Why This Works

`execution_timeline` + `platform_logs` allow Fluxbase to reconstruct execution **exactly as it occurred**, then let developers step through it like they're debugging locally — but they're actually inspecting a production request from hours ago.

No reproduction steps. No staging environment. No data export. **Pure replay debugging.**

---

## Incident Simulator: Validate Fixes Before Deploy

Once you can replay traffic and compare signatures, you can simulate fixes:

```bash
flux incident simulate \
  --window 2026-03-09T14:00..14:05 \
  --patch fix.js
```

**What happens:**

```
1. Extract incident traffic (127 requests in window)
2. Run original execution (baseline)
3. Apply patched code
4. Replay same 127 requests
5. Compare: errors, latencies, state mutations
```

**Output:**

```
Incident Simulation Report

Window: 2026-03-09 14:00-14:05
Requests: 127

BASELINE (production code a82d91a)
  Status 201 (success):  0 (0%)
  Status 500 (error):    127 (100%)
  Avg latency:          2500ms
  p95 latency:          4500ms

WITH FIX (patched code)
  Status 201 (success):  127 (100%)  ← FIX WORKS!
  Status 500 (error):    0 (0%)
  Avg latency:          850ms       ← Performance improved
  p95 latency:          1200ms

Detailed Comparison:

Request 1 (POST /checkout user=123)
  Before: ERROR stripe.charge timeout
  After:  SUCCESS status=201

Request 2 (POST /checkout user=456)
  Before: ERROR db connection lost
  After:  SUCCESS status=200

...

Summary:
  ✓ All 127 requests now succeed
  ✓ Latency improved (2500ms → 850ms)
  ✓ Ready to deploy with confidence
```

Then deploy knowing the fix actually works on real production traffic.

---

## Production Time-Travel: State Inspection at Any Time

### Reconstruct Backend State

Using `state_mutations` (append-only log) + periodic snapshots:

```bash
flux state inspect --at 2026-03-09T14:30:00Z
```

**Output:**

```
BACKEND STATE AT 2026-03-09 14:30:00 UTC

Snapshot: 2026-03-09T14:30:00 (used snapshot from 14:20:00 + replayed mutations)

users:
  u1:
    id: u1
    email: alice@example.com
    name: Alice
    created_at: 2026-03-09T14:02:00Z

  u2:
    id: u2
    email: bob@example.com
    name: Bob
    created_at: 2026-03-09T14:15:00Z

  u3:
    id: u3
    email: charlie@example.com
    name: Charlie
    created_at: 2026-03-09T14:29:00Z

orders:
  o1:
    id: o1
    user_id: u1
    total: 99.99
    status: completed
    created_at: 2026-03-09T14:03:00Z

  o2:
    id: o2
    user_id: u2
    total: 149.99
    status: pending
    created_at: 2026-03-09T14:25:00Z
```

### Compare State Between Two Times

```bash
flux state diff \
  --at 2026-03-09T14:00:00Z \
  --at 2026-03-09T14:05:00Z
```

**Output:**

```
CHANGES BETWEEN 14:00:00 AND 14:05:00

users: +3 rows inserted
  + u1 alice@example.com
  + u2 bob@example.com
  + u3 charlie@example.com

orders: +2 rows inserted
  + o1 total: 99.99
  + o2 total: 149.99

queue_jobs: +5 rows
  (async email jobs created)

Total mutations: 10
```

**Real-world use case:**

Event occurs: "Payments stopped processing 14:00-14:05"

```bash
flux state inspect --at 2026-03-09T13:59:00Z  # Before incident
flux state inspect --at 2026-03-09T14:05:00Z  # After incident

# Compare to see what state diverged
# Then trace which requests created those divergences
# Then debug those requests with flux trace debug
```

---

## State Blame: "Who Created This Record?"

### Link Records Back to Code

Find out exactly who created a database record and why:

```bash
flux state blame table:users id:u123
```

**Output:**

```
AUDIT TRAIL FOR users/u123

Record:
  id: u123
  email: alice@example.com
  name: Alice
  created_at: 2026-03-09T14:02:25Z

Created by request:
  request_id: 550e8400-e29b-41d4-a716-446655440000
  timestamp: 2026-03-09T14:02:25Z
  method: POST
  path: /api/users/create
  user: alice@example.com (jwt sub)

Function:
  name: create_user
  version: 7

Code:
  commit: a82d91a
  message: "feat: allow bulk user creation"
  author: dev@example.com
  date: 2026-03-08T10:15:00Z

  File: create_user/index.ts
  Lines 45-60:
    db.insert("users", {
      email: request.email,
      name: request.name,
      created_at: new Date()
    })

Trace:
  $ flux trace 550e8400
```

This is **Git blame for your database** — trace any record back to:

- The exact request that created it
- The exact commit that authorized it
- The exact code that modified it
- The exact time it was created

---

## The 10-Second Production Debugger

### Auto-Debug Command

The ultimate killer feature combines everything above:

```bash
flux debug
```

**Interactive session (no arguments):**

```
$ flux debug

Recent production errors (last 1h):

1. POST /checkout     db_timeout        127 errors   14:00-14:05
2. POST /signup       email_regex_error 3 errors     14:08-14:09
3. POST /login        jwt_decode_error  1 error      14:12

Select error to debug [1-3]:
> 2

Debugging: POST /signup email_regex_error (3 occurrences)

=== TRACE ===
Request: POST /signup
Status: 400 Bad Request
Duration: 45ms

Span tree:
  gateway.route (5ms)
  gateway.auth_passed (2ms)
  runtime.execute_function (38ms)
    ├─ email_validation.regex (8ms) ERROR: regex.test() failed
    ├─ [execution aborted]

=== EXECUTION CONTEXT ===
Locals at error:
  email = "josé@españa.es"
  regex = /^[a-zA-Z]+@[a-zA-Z]+\.[a-zA-Z]{2,}$/

=== CODE ANALYSIS ===
Likely cause:
  Email regex rejects international domains and special chars

Suggested fix:
  Change: /^[a-zA-Z]+@/
  To:      /^[a-zA-Z0-9._%+-]+@/

=== FIND ROOT CAUSE ===
Running automated bisect...

Testing a82d91a: ✓ (success)
Testing a83a02b: ✗ (error)

First bad commit: a83a02b
Author: dev
Message: "optimize email validation regex"
Date: 2026-03-08T16:45:00Z

Diff:
  - const EMAIL_REGEX = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
  + const EMAIL_REGEX = /^[a-zA-Z]+@[a-zA-Z]+\.[a-zA-Z]{2,}$/

=== REPLAY WITH FIX ===
Testing commit revert...

flux incident simulate --window 14:08..14:09 --patch <revert> 

Result:
  Errors before: 3
  Errors after: 0
  ✓ Fix validated

=== SUMMARY ===
Error Type: Regression
Introduced: a83a02b (2026-03-08T16:45:00Z)
Cause: Email regex too restrictive
Fix: Revert regex to original
Status: ✓ Validated on production traffic

Ready to deploy? (y/n)
> y

Deploying fix...
```

**Time to debug: ~10 seconds**. All automatic.

---

## The Complete Platform Vision

What you've built is a new category of backend platform:

| Capability | Purpose |
|-----------|---------|
| **Trace** | Know what happened |
| **Replay** | Reproduce execution (deterministically) |
| **Signatures** | Compare behavior (automatically) |
| **Bisect** | Find breaking commits (on real traffic) |
| **Timeline** | Step through execution |
| **Debugger** | Interactive production debugging |
| **Simulator** | Validate fixes on real traffic |
| **State Time-Travel** | Reconstruct backend at any moment |
| **State Blame** | Link records to code |
| **Auto-Debug** | 10-second bug diagnosis |

No other platform combines all of these.

Result: **Deterministic Backend Runtime** where every request is replayable, every bug is reproducible, and every deploy isvalidated against production data.

---

## Architecture Integration

**These features depend on:**

- `gateway.md` — Request envelope capture (trace_requests)
- `incident-replay` section of gateway.md — Sandbox execution & time-travel
- `platform_logs` — Complete trace tree with code_sha
- `state_mutations` — Backend state reconstruction (replay)
- `execution_state` — Local variable inspection (debug)
- `execution_timeline` JSONB — Checkpoint snapshots within requests

**These features enable:**

- `flux debug` — Auto-diagnosis of production bugs
- `flux trace debug` — Interactive step-through debugger
- `flux incident simulate` — Validate fixes before deploy
- `flux state inspect --at` — Backend time-travel
- `flux state blame` — Record accountability
- `flux bug bisect` — Automatic regression detection
- `flux guard deploy` — CI based on real traffic
- `flux trace diff` — Behavioral comparison
- `flux trace blame` — Code-level accountability

**These features depend on:**

- `gateway.md` — Request envelope capture (trace_requests)
- `incident-replay` section of gateway.md — Sandbox execution & time-travel
- `platform_logs` — Complete trace tree with code_sha
- `state_mutations` — Backend state reconstruction (replay)
- `execution_state` — Local variable inspection (debug)

**These features enable:**

- `flux bug bisect` — Automatic regression detection
- `flux guard deploy` — CI based on real traffic
- `flux trace diff` — Behavioral comparison
- `flux trace blame` — Code-level accountability
