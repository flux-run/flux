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

## Architecture Integration

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
