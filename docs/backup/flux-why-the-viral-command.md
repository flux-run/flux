# The Viral Command: `flux why`

## One Command That Changes Everything

```bash
$ flux why 550e8400-e29b-41d4-a716-446655440000

❌ request failed

┌─ ROOT CAUSE
│
├─ error: Stripe API timeout
│  location: payments/create.ts:42
│  caused by commit: a93f42c
│  changed code:
│    - timeout: 5000
│    + timeout: 5000 (no change visible — issue is upstream)
│
└─ FIX SUGGESTION
   latency regression: +230ms
   recommend: increase timeout to 8000 or add retry
   test with: flux incident simulate --patch fix.js
```

---

## Why This Command Is Viral

### For Developers (The Aha Moment)

**Before:**
```
Error in production. Spend 2 hours:
- Check logs (thousands of lines)
- Spin up local environment (30 min setup)
- Try to recreate issue (can't without production data)
- Dig through git blame (who changed what)
- Deploy fix (wait for deploy)
- Hope it works
```

**After:**
```
Error in production. 10 seconds:
$ flux why <request-id>

Boom. Here's the issue, the commit, the exact code
that broke, and what to change.
```

### For Operations (The Story)

`flux why` is not just a debugger — it's a **time machine**.

It says: "Here's what happened, when it happened, why it happened, and what changed to cause it."

This is the **only debugging capability** that makes sense in production because:
- It works with **real traffic** (not re-created scenarios)
- It shows **exact code** (not assumptions)
- It finds **root cause** (not symptoms)
- It suggests **fix** (not just problem)

---

## What's Behind `flux why`

It combines **10 capabilities** we designed:

```
Input: request-id (from response header)
         ↓
┌─────────────────────────────┐
│ 1. TRACE (what happened)    │  ← trace_requests + platform_logs
│────────────────────────────┐│
│ 2. REPLAY (reproduce it)    │ ← deterministic re-execution
│────────────────────────────┐│
│ 3. TIMELINE (where failed)  │ ← execution_state checkpoints
│────────────────────────────┐│
│ 4. BLAME (which commit)     │ ← code_sha in platform_logs
│────────────────────────────┐│
│ 5. BISECT (find breaking)   │ ← trace_signatures + binary search
│────────────────────────────┐│
│ 6. COMPARE (how broken)     │ ← signature_hash previous commits
│────────────────────────────┐│
│ 7. STATE (what changed)     │ ← state_mutations append log
│────────────────────────────┐│
│ 8. TIME (at what moment)    │ ← snapshots + mutation replay
│────────────────────────────┐│
│ 9. SUGGEST (how to fix)     │ ← latency regression alerts
│────────────────────────────┐│
│ 10. VALIDATE (will it work) │ ← incident simulator
└─────────────────────────────┘
         ↓
 Output: diagnosis + root cause + fix suggestion
```

**Key insight:** Each capability is **orthogonal** and **composable**.

- Want just the trace? `flux trace <id>`
- Want to replay? `flux trace replay <id>`
- Want to compare versions? `flux trace diff <a> <b>`
- Want everything? `flux why <id>`

---

## How `flux why` Works (Under the Hood)

### Step 1: Fetch Trace
```rust
// Read from trace_requests + platform_logs
SELECT request_id, method, path, headers, body, status_code, latency_ms
FROM trace_requests WHERE request_id = ?
JOIN platform_logs WHERE request_id = ?
```

### Step 2: Extract Code Provenance  
```rust
// All spans have code_sha (git commit when deployed)
SELECT code_sha, code_location, latency_ms
FROM platform_logs
WHERE request_id = ?
GROUP BY code_sha
```

### Step 3: Find Regression (Binary Search)
```rust
// Compare behavior (signature_hash) across commits
SELECT code_sha, signature_hash, status_code, latency_ms
FROM trace_signatures
WHERE function_id = ? AND created_at > NOW() - 7 DAYS
ORDER BY code_sha DESC

// Binary search: find commit where signature_hash changed
```

### Step 4: Identify Root Cause
```rust
// Compare code between working and broken version
$ git diff <good_sha>..<bad_sha> -- <code_location>
```

### Step 5: Suggest Fix
```rust
// Analyze: is it latency? error? logic?
IF latency_regression > 200ms AND calling_external_api:
  SUGGEST: "increase timeout or add retry"
ELSE IF error_type == "invalid_payload":
  SUGGEST: "schema changed — validate input"
ELSE IF error_type == "database_timeout":
  SUGGEST: "add index or optimize query"
```

### Step 6: Validate Fix
```bash
# Test the fix on real production traffic (no staging needed)
$ flux incident simulate --window 2026-03-10T14:00..14:30 --patch fix.js
```

---

## Why This Is Only Possible on Flux

### Other Platforms Can't Do This

| Platform | Can Trace? | Can Replay? | Can Bisect? | Can Compare? | **Total** |
|----------|-----------|-----------|-----------|-----------|---------|
| AWS Lambda | ❌ | ❌ | ❌ | ❌ | 0/4 |
| Vercel | ⚠️ | ❌ | ❌ | ❌ | 0.5/4 |
| Cloudflare Workers | ⚠️ | ❌ | ❌ | ❌ | 0.5/4 |
| Temporal | ⚠️ | ⚠️ | ❌ | ❌ | 1/4 |
| **Flux** | ✅ | ✅ | ✅ | ✅ | **4/4** |

**Why?** Flux owns the **entire stack**:

```
Request → [Gateway] → [Runtime] → [Data Engine] → [Database]
           ✅ capture    ✅ trace     ✅ log        ✅ mutation
           envelope    execution    state        append-log
```

AWS Lambda doesn't see load balancer decisions.  
Vercel doesn't control the database.  
Cloudflare doesn't control the runtime.

**Only Flux controls all layers.** So only Flux can offer:
- Request envelope capture (Gateway)
- Deterministic replay (Runtime)
- State reconstruction (Database)
- Code provenance (Deployment)

---

## Marketing Narrative

**The Tagline:**
> *Flux: Production debugging faster than local debugging*

**The Story:**
```
Before Flux:

  Local Development          Production
  ✅ Test works              ❌ Fails
  ✅ Can see logs            ⚠️ Logs are huge
  ✅ Can add breakpoints     ❌ Can't debug live data
  ✅ Can reproduce           ❌ Can't reproduce scenario
  ✅ Fix takes 5 min         ❌ Fix takes 2 hours

After Flux:

  $ flux why <request-id>
  
  [10 seconds later]
  
  Root cause + code + commit + fix suggestion
  
  Test with: $ flux incident simulate --patch fix.js
  
  Deploy with confidence.
```

**The Competition Response:**

- AWS Lambda: "We can do CloudWatch logs"
  - **Flux:** We do better with deterministic replay
  
- Vercel: "We have edge middleware"
  - **Flux:** We capture the full request-to-database journey
  
- Databricks: "We have MLflow tracing"
  - **Flux:** We trace every function call in production

**The Positioning:**

Flux is not just another serverless platform.  
Flux is **"Git for Backend Execution"** — you can time-travel, bisect, blame, and replay.

---

## Technical Requirements for `flux why`

### Database Tables (✅ All Ready)

```sql
trace_requests          -- request envelope (method, path, headers, body)
platform_logs           -- execution trace (code_sha, parent_span_id, execution_state)
state_mutations         -- state changes (before/after JSONB)
trace_signatures        -- behavioral fingerprint (latency, status, error)
```

### API Endpoints (⏳ To Implement)

```
GET  /traces/<id>               -- fetch trace
GET  /traces/<id>/replay        -- deterministic replay  
GET  /traces/<id>/blame         -- git blame for code
GET  /bisect                    -- binary search on commits
GET  /signatures/<id>/compare   -- compare signatures
POST /incident/simulate         -- validate fix on production traffic
```

### CLI Implementation (⏳ To Implement)

```bash
flux trace <id>                            # view execution
flux trace replay <id>                     # re-execute deterministically
flux trace blame <id>                      # git blame + code
flux trace debug <id>                      # step-through debugger
flux why <id>                              # 10-second diagnosis
flux incident simulate --patch fix.js      # validate fix on real traffic
```

---

## Launch Strategy

### Phase 1: Foundation (Now ✅)
- ✅ Database schema (trace_requests, state_mutations, trace_signatures)
- ✅ Gateway tracing (request envelope capture)
- ✅ Platform logs extension (code_sha, parent_span_id, execution_state)

### Phase 2: Integration (This Week)
- ⏳ Runtime: code_sha capture
- ⏳ Runtime: execution_state checkpoints
- ⏳ Data Engine: state_mutations logging

### Phase 3: Replay Engine (Next Week)
- ⏳ Deterministic replay service
- ⏳ Signature computation
- ⏳ Incident simulator

### Phase 4: CLI Commands (Week 3)
- ⏳ `flux trace` family
- ⏳ `flux debug` auto-diagnosis
- ⏳ `flux why` (combines all 10 capabilities)

### Phase 5: Production GA (Week 4-5)
- ⏳ Load testing
- ⏳ Security audit
- ⏳ Canary deployment
- ⏳ GA launch

---

## Success Metrics

`flux why` is **viral** when:

1. **Adoption:** 80% of developers use it weekly
2. **Time-to-resolution:** Average bug fix time drops 50% (2 hours → 1 hour)
3. **Production confidence:** Deploy frequency increases 3x
4. **Support reduction:** Debug-related support tickets drop 40%
5. **Word-of-mouth:** Developers recommend Flux for this feature

---

## Conclusion: The Aha Moment

Most debugging tools answer: **"What happened?"**

`flux why` answers: **"What happened, why it happened, when it happened, and how to fix it."**

In one command.  
In 10 seconds.  
Using only production data.

That's the **viral moment** — when a developer runs `flux why` once and realizes they'll never debug locally again.

---

## Next: Tell Me About the Streaming Response Question

You asked about streaming responses. Should the gateway:

A) **Stream responses** (forward hyper::Body directly)  
B) **Buffer responses** (collect entire body, then return)

**Answer:** Streaming.

For large file downloads (>10MB), streaming prevents OOM.  
For small JSON responses (typical), streaming has no benefit but also no harm.

Should we add a check to the verification checklist?

---

## One More Insight: Why This Architecture Is Bulletproof

The **canonical truth** flows one direction:

```
Request → [trace_requests] → [execution] → [responses]
            ↓
         [mutations log]
            ↓
         [state snapshots]
```

If you ever need to debug, you have:
- Complete input (trace_requests)
- Complete execution trace (platform_logs with checkpoints)
- Complete state changes (state_mutations)
- Exact code version (code_sha)

Nothing is lost. Nothing is uncertain. Everything is queryable.

This is why Flux can promise: **"Production debugging faster than local debugging."**

Because in production, you have MORE information than local.

