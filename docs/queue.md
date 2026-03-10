# Queue Service

The Fluxbase Queue service is a standalone async job execution engine. It accepts job submissions, persists them in PostgreSQL, and dispatches them to the Runtime service for execution — with retries, timeout recovery, idempotency, and observability built in.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Delivery Guarantee](#delivery-guarantee)
3. [Data Model](#data-model)
4. [Job Lifecycle](#job-lifecycle)
5. [Worker System](#worker-system)
6. [Worker Fairness & Tenant Isolation](#worker-fairness--tenant-isolation)
7. [Retry & Backoff](#retry--backoff)
8. [Timeout Recovery & Visibility Timeout Model](#timeout-recovery--visibility-timeout-model)
9. [Idempotency](#idempotency)
10. [HTTP API](#http-api)
11. [Stats & Observability](#stats--observability)
12. [Configuration](#configuration)
13. [Deployment](#deployment)
14. [Architecture Scorecard](#architecture-scorecard)
15. [Roadmap](#roadmap)
16. [Known Issues & Improvement Areas](#known-issues--improvement-areas)

---

## Architecture Overview

```
API Service ──POST /jobs──► Queue Service HTTP API
                                    │
                             Postgres (jobs table)
                                    │
                     ┌──────────────┘
                     │  poller loop (every 200 ms)
                     │  SELECT ... FOR UPDATE SKIP LOCKED
                     │  Semaphore (50 concurrent workers)
                     │
                 tokio::spawn ×N
                     │
              Runtime Service
              POST /internal/execute
                     │
              ┌──────┴──────┐
           success        failure
              │              │
        status=completed  retry / dead letter

Background loop (every 30 s):
  Timeout Recovery — rescues stuck "running" jobs
```

The service is a single Tokio process that runs two concurrent loops:

| Loop | Purpose |
|------|---------|
| **Worker poller** | Fetches and dispatches pending jobs to the runtime |
| **Timeout recovery** | Rescues jobs that exceeded `max_runtime_seconds` |

The HTTP server (Axum) runs alongside both loops in the same process.

---

## Delivery Guarantee

The queue provides **at-least-once delivery**. A job may execute more than once in the following cases:

- The worker crashes after calling the runtime but before marking the job `completed`.
- The timeout recovery loop resets a job to `pending` while the original worker is still running (slow but not dead).
- A manual retry is issued against a job that already completed.

Jobs that must not execute twice must be made idempotent on the function side. Callers can also use `idempotency_key` to prevent duplicate *enqueue* — but that does not prevent duplicate *execution*.

---

## Data Model

### `jobs` (primary queue table)

| Column | Type | Default | Description |
|--------|------|---------|-------------|
| `id` | UUID | `gen_random_uuid()` | Primary key |
| `tenant_id` | UUID | — | Owning tenant |
| `project_id` | UUID | — | Owning project |
| `function_id` | UUID | — | Function to invoke |
| `payload` | JSONB | — | Arbitrary input passed to the function |
| `status` | TEXT | `'pending'` | Current status (see Job Status) |
| `attempts` | INT | `0` | Number of execution attempts so far |
| `max_attempts` | INT | `5` | Maximum attempts before dead-lettering |
| `max_runtime_seconds` | INT | `300` | Max seconds a job may stay in `running` |
| `run_at` | TIMESTAMP | `now()` | Earliest time a worker may pick up this job |
| `locked_at` | TIMESTAMP | NULL | When the row was claimed by a worker |
| `started_at` | TIMESTAMP | NULL | When the HTTP call to the runtime began |
| `idempotency_key` | TEXT | NULL | Optional deduplication key (unique index) |
| `created_at` | TIMESTAMP | `now()` | Creation time |
| `updated_at` | TIMESTAMP | `now()` | Last mutation time |

> **Planned additions (migration required):**
>
> | Column | Type | Purpose |
> |--------|------|---------|
> | `request_id` | UUID | Link async job execution to the originating request trace |
> | `parent_span_id` | UUID | Attach job span to the trace tree started by the caller |
> | `code_sha` | TEXT | Git SHA of the deployed function at enqueue time — enables exact reproduction during incident replay |
> | `priority` | INT | Worker poll order; higher values are dispatched first (default `0`) |
> | `queue_name` | TEXT | Logical queue name (default `'default'`); enables billing, email, background queues without separate services |
> | `enqueue_span_id` | UUID | Span ID generated at enqueue time; creates an explicit `queue.enqueue` node in the trace tree so the gap between request and execution is visible |
> | `result` | JSONB | Runtime output; enables `GET /jobs/:id` to return full execution result |
> | `error_detail` | TEXT | Structured error from the last failed attempt |
>
> Without `request_id` and `parent_span_id`, `flux trace`, `flux why`, and `flux replay` cannot link async jobs back to the request that triggered them.

**Indexes:**
- `idx_jobs_pending` on `(status, run_at)` — used by the poller query
- `idx_jobs_stuck` on `(status, locked_at) WHERE status = 'running'` — used by timeout recovery
- `idx_jobs_idempotency_key` on `(idempotency_key) WHERE idempotency_key IS NOT NULL` (unique)

### `job_logs` (append-only audit trail)

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID | Primary key |
| `job_id` | UUID | Foreign key → `jobs.id` ON DELETE CASCADE |
| `message` | TEXT | Human-readable event description |
| `created_at` | TIMESTAMP | When the event was recorded |

Logged events: `job started`, `job completed`, `job failed: …`, `retry scheduled (attempt N)`, `retry limit reached, moved to dead letter`, `timed out — reset to pending (attempt N)`.

### `dead_letter_jobs` (terminal failures)

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID | Copied from `jobs.id` |
| `tenant_id` | UUID | — |
| `project_id` | UUID | — |
| `function_id` | UUID | — |
| `payload` | JSONB | Original payload |
| `error` | TEXT | Failure reason |
| `failed_at` | TIMESTAMP | When the job was dead-lettered |

When a job is dead-lettered it is **removed** from `jobs` and **inserted** into `dead_letter_jobs`.

---

## Job Lifecycle

```
             ┌─────────┐
  POST /jobs │ pending │◄──────────────────────────────┐
             └────┬────┘                               │
                  │  worker poll (FOR UPDATE SKIP LOCKED)
                  ▼                                    │
            ┌─────────┐   timeout recovery (attempts < max)
            │ running │──────────────────────────────►─┘
            └─────┬───┘
         ┌────────┴────────┐
    HTTP 2xx           HTTP error / network error
         │                  │
    ┌────┴────┐    attempts < max_attempts?
    │completed│       Yes ──► pending (with backoff delay)
    └─────────┘       No  ──► dead_letter_jobs
                            (also via timeout exhaustion)

  Manual flows:
    DELETE /jobs/:id  → cancelled (status update only, not removed)
    POST /jobs/:id/retry → pending (attempts reset to 0)
```

### Status values

| Status | Meaning |
|--------|---------|
| `pending` | Waiting to be picked up by a worker |
| `running` | Claimed by a worker; HTTP call in flight |
| `completed` | Runtime returned 2xx |
| `failed` | Intermediate state after an unsuccessful attempt with retries remaining (reset to `pending` quickly) |
| `cancelled` | Manually cancelled via API; workers will not pick it up |
| `dead` | Exhausted all retries (maps to the `dead_letter_jobs` table) |

---

## Worker System

**File:** `queue/src/worker/poller.rs`, `worker/executor.rs`

### Fetch-and-lock query

The poller issues an atomic UPDATE per batch to avoid races between multiple queue instances:

```sql
UPDATE jobs SET status = 'running', locked_at = now(), updated_at = now()
WHERE id IN (
    SELECT id FROM jobs
    WHERE status = 'pending' AND run_at <= now()
    ORDER BY run_at
    LIMIT 20
    FOR UPDATE SKIP LOCKED
)
RETURNING *
```

`FOR UPDATE SKIP LOCKED` ensures two poller instances never claim the same job. Batch size is **20 jobs per poll tick**.

### Concurrency control

A `tokio::sync::Semaphore` (size = `WORKER_CONCURRENCY`, default 50) limits the number of in-flight HTTP calls to the runtime. Each job task holds a semaphore permit for its entire lifetime and releases it on completion.

### Execution flow (per job)

1. Stamp `started_at = now()` on the job row
2. POST to `{RUNTIME_URL}/internal/execute` with `{ function_id, payload }`
3. **On 2xx** → `status = 'completed'`; log `job completed`
4. **On non-2xx or network error** → call `handle_failure`:
   - If `attempts + 1 < max_attempts`: schedule retry (back to `pending` with exponential delay)
   - Otherwise: dead-letter the job

> **Planned:** Pass `request_id` and `parent_span_id` as headers on the runtime POST so execution spans are attached to the originating trace.

---

## Worker Fairness & Tenant Isolation

The current poller fetches the oldest `pending` jobs globally (`ORDER BY run_at`) with no tenant awareness. This means a single tenant enqueuing a large batch can starve all other tenants.

**Example:**
```
Tenant A → 1,000,000 pending jobs
Tenant B →          10 pending jobs
```
Tenant B's jobs may not execute for minutes even though they are time-sensitive.

### Fix options (in order of complexity)

**Option A — Fair ordering (minimal change)**

Change the inner SELECT in the fetch query:

```sql
ORDER BY tenant_id, run_at
```

This interleaves tenants in the batch rather than processing one tenant's entire backlog before moving to the next. Simple to implement; no schema change needed.

**Option B — Named queues (medium)**

Add a `queue_name TEXT DEFAULT 'default'` column. Run separate pollers per queue (e.g. `high`, `default`, `low`). Callers assign jobs to queues based on priority tier. Each queue can have its own concurrency limit.

**Option C — Per-tenant worker pools (long term)**

Allocate a dedicated semaphore (or worker process) per tenant tier. Enterprise tenants get isolated capacity. This mirrors Temporal's task queue model and is the right architecture once Fluxbase has tiered billing.

**Recommended now:** Option A. It is a one-line change that eliminates the worst-case starvation scenario with no migration.

---

## Retry & Backoff

**File:** `queue/src/worker/backoff.rs`

Retry delay is **exponential**: `5s × 2^attempts`

| Attempt | Delay |
|---------|-------|
| 1 | 5 s |
| 2 | 10 s |
| 3 | 20 s |
| 4 | 40 s |
| 5+ | 80 s… |

The retry sets `run_at = now() + delay` so the job is invisible to pollers until the delay elapses.

`max_attempts` is set to **5** at job creation time (hardcoded in the create-job handler).

---

## Timeout Recovery & Visibility Timeout Model

**File:** `queue/src/worker/timeout_recovery.rs`

A background loop runs every `JOB_TIMEOUT_CHECK_INTERVAL_MS` (default 30 s).

### What counts as stuck?

```sql
status = 'running'
AND started_at IS NOT NULL
AND started_at + (max_runtime_seconds * interval '1 second') < now()
```

`started_at` is used (not `locked_at`) because it marks when execution actually began. The gap between `locked_at` and `started_at` is the overhead of the Tokio spawn + semaphore wait, which should not count against job runtime.

### Recovery logic

The recovery runs atomically:

```sql
UPDATE jobs
SET attempts = attempts + 1, locked_at = NULL, started_at = NULL, updated_at = now()
WHERE status = 'running'
  AND started_at IS NOT NULL
  AND started_at + (max_runtime_seconds * interval '1 second') < now()
RETURNING id, attempts, max_attempts
```

For each returned row:
- `attempts < max_attempts` → reset `status = 'pending'` (worker will retry)
- `attempts >= max_attempts` → copy to `dead_letter_jobs` with error `"timed out after max attempts"`, then delete from `jobs`

### Relationship to the visibility timeout pattern

Systems like SQS and BullMQ use a concept called a **visibility timeout**: a worker picks up a job and the job becomes invisible to other workers for N seconds. If the worker does not ACK within that window, the job reappears automatically.

This queue approximates that pattern:

| SQS / BullMQ concept | This queue equivalent |
|---------------------|-----------------------|
| Message visibility timeout | `max_runtime_seconds` (default 300 s) |
| Implicit lease / heartbeat | None — lease is held until `started_at + max_runtime_seconds` elapses |
| Auto-requeue on lease expiry | `timeout_recovery` loop (runs every 30 s) |
| ACK | `UPDATE jobs SET status = 'completed'` |

The key difference: this queue has a **coarse recovery interval** (30 s). A stuck job may remain in `running` for up to 30 s after its `max_runtime_seconds` window closes before recovery resets it. If sub-second visibility timeout semantics are needed, the check interval should be reduced or the recovery query should run inside the worker on task completion.

### Target trace tree (once trace fields are implemented)

```
gateway.request
  └─ runtime.function
        └─ queue.enqueue          ← enqueue_span_id stamps this node
              └─ worker.execution ← parent_span_id = enqueue_span_id
                    └─ runtime.function
                          └─ db mutation
```

Without `enqueue_span_id` the gap between `queue.enqueue` and `worker.execution` is invisible in the trace tree — you see the request and the execution but not how long the job sat in the queue.

---

## Idempotency

Callers may supply an `idempotency_key` string. The insert uses:

```sql
INSERT INTO jobs (..., idempotency_key)
VALUES (..., $7)
ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL
DO UPDATE SET updated_at = jobs.updated_at
RETURNING id
```

If a job with the same key already exists, the existing `job_id` is returned and no duplicate is created. This is safe to call multiple times (e.g., on network retries from the API layer).

---

## HTTP API

Base path: `/`  
No authentication middleware is currently applied.

### `POST /jobs`

Create a new job.

**Request body** (JSON):

```json
{
  "tenant_id": "uuid",
  "project_id": "uuid",
  "function_id": "uuid",
  "payload": { "any": "json" },
  "idempotency_key": "optional-string"
}
```

**Response** `200`:

```json
{ "job_id": "uuid" }
```

---

### `GET /jobs/:id`

Fetch a job by ID. In addition to all model fields, two derived timing metrics are returned:

| Field | Calculation |
|-------|-------------|
| `queue_time_ms` | `started_at - created_at` (time spent waiting in queue) |
| `execution_time_ms` | `updated_at - started_at` (approximation of runtime duration) |

Both are `null` if the job has not started yet.

**Response** `200`:

```json
{
  "id": "uuid",
  "status": "completed",
  "attempts": 1,
  "queue_time_ms": 183,
  "execution_time_ms": 1204,
  ...
}
```

---

### `DELETE /jobs/:id`

Cancel a pending or running job (sets `status = 'cancelled'`). Does not stop an in-flight worker — the runtime call may still complete, but the status will remain `cancelled`.

**Response** `200`:

```json
{ "status": "cancelled" }
```

---

### `POST /jobs/:id/retry`

Manually re-queue a failed/cancelled/dead job. Resets attempts to `0` and sets `run_at = now() + 5s`.

**Response** `200`:

```json
{ "status": "retried" }
```

---

### `GET /jobs/stats`

Aggregate statistics across all jobs.

**Response** `200`:

```json
{
  "queue": {
    "pending": 12,
    "running": 3,
    "completed": 4891,
    "failed": 0,
    "cancelled": 5,
    "dead_letter": 2
  },
  "latency_ms": {
    "avg_queue_time": 145,
    "p95_queue_time": 820,
    "avg_execution_time": 1102,
    "p95_execution_time": 4300
  },
  "retries": {
    "total": 18,
    "jobs_retried": 11,
    "max_seen": 3
  }
}
```

Latency metrics are computed only for `completed` jobs with `started_at` set.

---

### `GET /health`

```json
{ "status": "ok" }
```

### `GET /version`

```json
{
  "service": "queue",
  "commit": "abc1234",
  "build_time": "2026-03-11T09:00:00Z"
}
```

---

## Stats & Observability

- All significant job events are written to `job_logs` with a timestamped message.
- Structured tracing (via `tracing` crate) emits `job_id` and `function_id` on every log line.
- `RUST_LOG` / `RUST_LOG_STYLE` control log level; default filter is `queue=debug,tower_http=debug`.

> **Planned:** Expose Prometheus counters on `/metrics`:
> ```
> queue_jobs_total{status="completed|failed|dead"}
> queue_jobs_retried_total
> queue_jobs_dead_total
> queue_job_queue_time_ms (histogram)
> queue_job_execution_time_ms (histogram)
> ```

---

## Configuration

All configuration is via environment variables (loaded from `.env` or `env.yaml` in Cloud Run).

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *(required)* | PostgreSQL connection string |
| `RUNTIME_URL` | `http://localhost:3002` | Base URL of the Runtime service |
| `PORT` / `QUEUE_PORT` | `8080` | HTTP server port |
| `WORKER_CONCURRENCY` | `50` | Max concurrent in-flight runtime calls |
| `WORKER_POLL_INTERVAL_MS` | `200` | How often the poller fetches new jobs |
| `JOB_TIMEOUT_CHECK_INTERVAL_MS` | `30000` | How often stuck-job recovery runs |
| `GIT_SHA` | `unknown` | Injected at build time, surfaced on `/version` |
| `BUILD_TIME` | `unknown` | Injected at build time, surfaced on `/version` |

---

## Deployment

The service is packaged as a Docker image and deployed to Cloud Run.

```bash
# Build and deploy (from workspace root)
make deploy-gcp SERVICE=queue

# Run migrations manually before first deploy or when adding migration files
make migrate SERVICE=queue
```

Dockerfile uses a two-stage Rust build (`rust:1.93-bookworm` builder → `debian:bookworm-slim` runtime). `SQLX_OFFLINE=true` is set so the binary compiles without a live DB.

> **Note:** `db::connection::migrate(&pool)` is commented out in `main.rs`. Migrations must be applied explicitly (`make migrate`) — this avoids startup hangs on Neon serverless DB cold-starts where `pg_advisory_lock` can block indefinitely.

---

## Architecture Scorecard

| Area | Score | Notes |
|------|-------|-------|
| Queue architecture | 9.5 / 10 | Standard Postgres SKIP LOCKED pattern, proven at scale |
| Retry logic | 10 / 10 | Correct exponential backoff, prevents retry storms |
| Timeout recovery | 10 / 10 | Rescues stuck jobs; uses `started_at` not `locked_at` |
| Idempotency | 10 / 10 | `ON CONFLICT` deduplication eliminates duplicate enqueue |
| Data model | 8.5 / 10 | Solid; missing trace columns (`request_id`, `parent_span_id`, `code_sha`) |
| Tracing integration | 3 / 10 | No `request_id` propagation; async jobs invisible to `flux trace` / `flux why` |
| Multi-tenant fairness | 5 / 10 | Global FIFO; noisy tenant can starve others |
| Observability | 6 / 10 | Job logs present; no metrics endpoint, no DLQ alerting |

**Overall: production-capable today. Adding trace propagation and Option A fairness brings it to full platform alignment.**

---

## Roadmap

Items are ordered by impact. All schema changes require a new migration file.

### P0 — Required for platform alignment

**1. Trace propagation**

Add four columns to `jobs`:

```sql
ALTER TABLE jobs
  ADD COLUMN request_id      UUID,
  ADD COLUMN parent_span_id  UUID,
  ADD COLUMN enqueue_span_id UUID,
  ADD COLUMN code_sha        TEXT;
```

Populate them at enqueue time (pass through from the calling API request). `enqueue_span_id` is generated fresh on each `POST /jobs` call — it represents the `queue.enqueue` node in the trace tree. Forward `x-request-id` and `x-span-id` (set to `enqueue_span_id`) as headers on the runtime POST so the worker execution span attaches to the correct parent. This unblocks `flux trace`, `flux why`, and `flux replay` for async workflows.

**2. Worker fairness (Option A)**

One-line change in `queue/src/queue/fetch_jobs.rs`: add `tenant_id` to the inner `ORDER BY` clause to interleave tenants in each polled batch.

### P1 — High value

**3. Job result storage**

```sql
ALTER TABLE jobs
  ADD COLUMN result       JSONB,
  ADD COLUMN error_detail TEXT;
```

Store runtime output on `completed` and the error body on `failed`/`dead`. `GET /jobs/:id` then returns the full execution result without requiring callers to store it themselves.

**4. Named queues**

```sql
ALTER TABLE jobs ADD COLUMN queue_name TEXT NOT NULL DEFAULT 'default';
CREATE INDEX idx_jobs_pending_queue ON jobs(queue_name, status, priority DESC, run_at)
  WHERE status = 'pending';
```

Allow callers to assign jobs to logical queues (`'billing'`, `'email'`, `'background'`, `'high'`). Each queue can be polled with a separate concurrency limit or even a separate worker process — no additional services required. This is the foundation for Option B tenant fairness and priority tiers.

**5. Job priority**

```sql
ALTER TABLE jobs ADD COLUMN priority INT NOT NULL DEFAULT 0;
CREATE INDEX idx_jobs_pending_priority ON jobs(queue_name, status, priority DESC, run_at)
  WHERE status = 'pending';
```

Update the inner `ORDER BY` to `priority DESC, run_at`. Combined with `queue_name`, this enables fine-grained dispatch without separate services.

**6. State mutation log (Fluxbase-specific)**

For time-travel debugging (`flux state blame`, `flux incident replay`) async jobs must also record state mutations:

```sql
CREATE TABLE job_state_mutations (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id     UUID REFERENCES jobs(id) ON DELETE CASCADE,
  request_id UUID,
  mutation   JSONB NOT NULL,
  ts         TIMESTAMP NOT NULL DEFAULT now()
);
```

The runtime writes one row per state-mutating operation during job execution. This gives the same replay capability as synchronous requests.

### P2 — Operational quality

**7. Prometheus metrics endpoint** — `/metrics` with counters and histograms listed in the Stats section above.

**8. DLQ alerting** — webhook or Cloud Pub/Sub notification when a job enters `dead_letter_jobs`.

**9. Job list endpoint** — `GET /jobs?status=&tenant_id=&limit=&cursor=` for dashboard visibility.

**10. Scheduled/future jobs** — expose `run_at` in `POST /jobs` so callers can delay job execution.

**11. `max_attempts` per request** — remove the hardcoded `5`; accept it in `CreateJobRequest` with `5` as the default.

---

## Known Issues & Improvement Areas

### Functional gaps

| Issue | Location | Fix |
|-------|----------|-----|
| `max_attempts` hardcoded to `5` | `api/handlers/create_job.rs` | Add to `CreateJobRequest`; default to `5` |
| `run_at` always `now()` | `api/handlers/create_job.rs` | Accept `run_at` in request body for scheduled jobs |
| Manual retry resets `attempts = 0` | `api/handlers/retry_job.rs` | Preserve attempt history; only reset `run_at` and `status` |
| `scheduler.rs` is empty | `worker/scheduler.rs` | Implement cron/recurring job support or remove the file |
| No auth on endpoints | `api/routes.rs` | Add internal service token check (pattern already exists in data-engine) |
| No job list endpoint | — | Add `GET /jobs?status=&tenant_id=&cursor=` |
| Cancelled jobs accumulate forever | `queue/fetch_jobs.rs` | Add a periodic `DELETE FROM jobs WHERE status = 'cancelled' AND updated_at < now() - interval '7 days'` |

### Code quality

| Issue | Location | Fix |
|-------|----------|-----|
| `lock_job.rs` is unused | `queue/lock_job.rs` | Delete; locking is atomic inside `fetch_and_lock_jobs` |
| `utils/backoff.rs` re-exports `worker::backoff` | `utils/backoff.rs` | Remove the wrapper; call `worker::backoff::retry_delay` directly |
| Dead-letter deletes the source row | `services/retry_service.rs` | Consider soft-delete (`status = 'dead'`) to preserve attempt history |
| No per-tenant isolation in poller | `queue/fetch_jobs.rs` | Apply Option A (add `tenant_id` to `ORDER BY`) — see Worker Fairness section |
| `job_service.rs` ignores `run_at` from callers | `api/handlers/create_job.rs` | Thread `run_at` from request through `CreateJobInput` |

### Observability gaps

- No `request_id` / `parent_span_id` propagation — async jobs are invisible to `flux trace` and `flux why` (see Roadmap P0)
- `queue_time_ms` and `execution_time_ms` are derived on read but never stored or emitted as metrics
- Dead-letter jobs have no notification mechanism (webhook, alert, Cloud Pub/Sub topic)
- Timeout recovery counter (`count = N stuck jobs rescued`) is logged but not exposed as a metric
