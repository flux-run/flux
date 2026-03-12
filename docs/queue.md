# Queue Service

> **Internal architecture doc.** This describes the Queue service implementation
> for contributors. For user-facing docs, see
> [framework.md §15](framework.md#15-queue).

---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-queue` |
| Role | Async job execution with retries, timeout recovery, tracing |
| Tech | Rust, Axum, SQLx, PostgreSQL |
| Default port | `:8084` |
| Exposed to internet | No — receives traffic from Runtime and API only |

The Queue service accepts job submissions from any Flux service, persists them
in Postgres, and dispatches them to the Runtime for execution. It never
executes user code directly — all execution goes through the Runtime.

```
Runtime :8083
     │  ctx.queue.push("send_email", payload)
     ▼
Queue :8084
     ├── HTTP API (job submission)
     ├── Postgres (durable job storage)
     ├── Poller (every 200ms, FOR UPDATE SKIP LOCKED)
     ├── Worker pool (bounded semaphore, 50 concurrent)
     │
     └── Dispatches back to Runtime :8083 for execution
```

---

## Why the Queue never executes code

The Queue delegates all execution to the Runtime. This ensures:

- **Sandbox isolation** — user code always runs in Deno V8 isolates
- **Consistent tracing** — every execution emits spans through the Runtime tracing path
- **Deterministic replay** — `flux queue replay` re-runs via Runtime with same `code_sha`
- **Tool access** — `ctx.tools`, `ctx.workflow` only available inside Runtime

---

## Delivery guarantee

**At-least-once delivery.** Jobs are acknowledged only after successful
execution. Failed jobs are retried with exponential backoff.

---

## Job lifecycle

```
PENDING → RUNNING → COMPLETED
                  → FAILED → RETRY → RUNNING → ...
                  → DEAD_LETTER (max retries exceeded)
```

| State | Description |
|---|---|
| `pending` | Waiting for worker pickup |
| `running` | Worker acquired, executing via Runtime |
| `completed` | Successful execution |
| `failed` | Execution failed, will retry |
| `dead_letter` | Max retries exceeded, requires manual intervention |

---

## Worker system

```
Poller thread (every 200ms)
     │
     ▼
SELECT id, function_name, payload, ...
FROM jobs
WHERE status = 'pending' AND scheduled_at <= NOW()
FOR UPDATE SKIP LOCKED
LIMIT 10
     │
     ▼
Semaphore (50 concurrent workers)
     │
     ▼
POST /execute → Runtime :8083
```

- `FOR UPDATE SKIP LOCKED` prevents double-pickup in multi-instance deployments
- Bounded semaphore prevents worker explosion under load
- Each worker dispatches to Runtime and waits for completion

---

## Retry & backoff

| Config | Default |
|---|---|
| Max retries | 3 |
| Base delay | 1 second |
| Backoff multiplier | 2× |
| Max delay | 60 seconds |
| Jitter | ±25% |

Retry formula: `delay = min(base × 2^attempt + jitter, max_delay)`

---

## Timeout recovery

Jobs have a visibility timeout (default: 5 minutes). If a worker doesn't
acknowledge completion within the timeout, the job returns to `pending` state.

This handles:
- Worker crashes
- Network partitions between Queue and Runtime
- Runtime OOM kills

---

## Idempotency

Optional `idempotency_key` on job submission:

```typescript
await ctx.queue.push("send_email", { user_id: "123" }, {
  idempotencyKey: "welcome-email-123"
});
```

If a job with the same key exists (any status except `dead_letter`), the push
is a no-op. Prevents duplicate jobs from retries or race conditions.

---

## Tracing integration

Queue jobs participate in the same distributed trace system:

```
gateway.request           ← original request_id
  └─ runtime.function
        └─ queue.enqueue       ← enqueue span
              └─ queue.wait        ← time in queue
                    └─ worker.execution
                          └─ runtime.function    ← async function execution
                                └─ db mutation
```

This makes async jobs visible to `flux trace`, `flux why`, and
`flux incident replay`.

---

## HTTP API

| Endpoint | Method | Description |
|---|---|---|
| `/jobs` | `POST` | Submit a job |
| `/jobs` | `GET` | List jobs (with status filter) |
| `/jobs/:id` | `GET` | Get job details |
| `/jobs/:id/retry` | `POST` | Retry a failed/dead job |
| `/jobs/:id/cancel` | `POST` | Cancel a pending job |
| `/stats` | `GET` | Queue statistics (pending, running, failed counts) |
| `/health` | `GET` | Health check |

All endpoints require `X-Service-Token`.

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `PORT` | `8084` | HTTP listen port |
| `DATABASE_URL` | — | Postgres for job storage |
| `RUNTIME_URL` | `http://localhost:8083` | Runtime for job execution |
| `INTERNAL_SERVICE_TOKEN` | — | Service-to-service auth |
| `POLL_INTERVAL_MS` | `200` | Poller frequency |
| `MAX_CONCURRENT_WORKERS` | `50` | Worker pool size |
| `DEFAULT_TIMEOUT_SECS` | `300` | Visibility timeout |
| `MAX_RETRIES` | `3` | Default retry count |

---

*Source: `queue/src/`. For the queue spec, see
[framework.md §15](framework.md#15-queue).*
