# Decisions: Storage, Database Scope & Record Retention

**Date:** 2026-03-13
**Impact:** Architecture — defines what Flux does NOT build + retention model

---

## Summary

Three decisions that set boundaries on what Flux owns:

| Decision | Answer |
|---|---|
| Built-in storage (S3/GCS/R2)? | **No** — functions + SDK |
| Multiple databases per instance? | **No** — one binary, one port, one DB |
| Archive old records to S3? | **No** — delete on schedule, export first if needed |

---

## 1. No Storage Primitive

Flux does NOT provide a built-in storage system. No S3 abstraction, no file upload endpoint, no bucket management.

**Why:**
- Flux's moat is execution recording, not breadth-of-services
- S3/GCS/R2 SDKs are 3 lines inside a function — no insight gap to fill
- Storage calls are already captured as `ExternalCall` in the execution trace
- Every abstraction over cloud storage leaks (presigned URLs, CORS, CDN invalidation, multipart)

**Pattern:** Write a function, use the SDK. Flux records the call automatically.

```typescript
// functions/upload_avatar/index.ts
import { S3Client, PutObjectCommand } from "@aws-sdk/client-s3";

export default defineFunction({
  name: "upload_avatar",
  handler: async ({ input, ctx }) => {
    const s3 = new S3Client({ region: ctx.secrets.get("AWS_REGION") });
    await s3.send(new PutObjectCommand({
      Bucket: ctx.secrets.get("S3_BUCKET"),
      Key: `avatars/${input.userId}`,
      Body: input.file,
    }));
    return { url: `https://${ctx.secrets.get("S3_BUCKET")}.s3.amazonaws.com/avatars/${input.userId}` };
  },
});
```

**Code impact:** None. Don't build anything. If storage routes exist in the API, delete them.

---

## 2. One Database Per Instance

One Flux instance = one Postgres database. Period.

**Why:**
- Matches single-binary philosophy: one binary, one port, one DB
- Application tables + Flux internal tables (`execution_records`, `execution_spans`, `execution_mutations`, `execution_calls`) share one database, one `PgPool`, one `DATABASE_URL`
- Multiple databases = multiple connection pools, multiple migration tracks, ambiguity about which `ctx.db` points to
- Rails shipped with one DB for 15 years — enough for 99% of apps

**If someone needs a second database:** They connect to it inside their function with a Postgres client. Flux doesn't manage it, but records the call as `ExternalCall`.

**Code impact:**
- Ensure `ctx.db` always points to the single configured database
- No `[databases]` section in `flux.toml` — just `DATABASE_URL` env var
- No multi-database support in Data Engine
- If any multi-tenant database routing exists, remove it

---

## 3. Record Retention: Delete, Don't Archive

Old execution records are **hard-deleted** from Postgres on a schedule. Flux does NOT back them up to S3 or any external storage.

**Why:**
- We just decided "no storage primitive" — building S3 archive into the framework contradicts that
- 99% of teams don't need 90-day-old traces
- If someone needs archival, it's a function (export → pipe to wherever)

### Config

```toml
# flux.toml
[observability]
record_retention_days = 30    # delete successful records older than 30 days
error_retention_days  = 90    # errors kept 3x longer (default: 3x record_retention_days)
retention_job_hour    = 3     # hour (UTC) to run daily cleanup
```

### What to build

**Retention job (Data Engine background task):**
- Runs daily at `retention_job_hour` (default 3am UTC)
- Deletes from all 4 tables: `execution_records`, `execution_spans`, `execution_mutations`, `execution_calls`
- Errors use `error_retention_days`, everything else uses `record_retention_days`
- Uses batched deletes (e.g., 1000 rows per batch) to avoid long locks
- Logs: `"Retention: deleted 12,847 records older than 30 days"`

**New CLI commands — `flux records`:**

| Command | What it does |
|---|---|
| `flux records export [--before 30d] [--after 7d] [--function name] [--errors-only] [--format jsonl\|csv]` | Stream records as JSONL (default) or CSV to stdout |
| `flux records count [--before 30d] [--after 7d] [--function name]` | Count matching records (preview what retention will delete) |
| `flux records prune [--before 30d] [--dry-run]` | Manually delete old records on demand |

**Export format (JSONL):** One JSON object per line, each object is a complete `ExecutionRecord` with nested spans, mutations, and calls. This is the same shape as the `ExecutionRecord` interface in §3 of framework.md.

```bash
# User exports, user handles storage
flux records export --before 30d > records-2026-03.jsonl
flux records export --before 30d | aws s3 cp - s3://my-bucket/flux-archive/2026-03.jsonl
flux records export --before 30d --errors-only > errors-2026-03.jsonl
```

### Implementation notes

- Retention job should be a tokio task spawned by the Data Engine module at startup
- Use `DELETE FROM execution_records WHERE started_at < $1 AND error IS NULL LIMIT 1000` in a loop
- For errors: `DELETE FROM execution_records WHERE started_at < $1 AND error IS NOT NULL LIMIT 1000`
- Cascade deletes to spans/mutations/calls via `ON DELETE CASCADE` foreign keys
- `flux records export` queries with cursor pagination, streams to stdout — never loads all records into memory
- `flux records prune --dry-run` just runs `flux records count` with the same filters

---

## The Rule

**Flux owns things it needs to record intelligently: functions, database, queue.**

Everything else (storage, email, payments, extra databases) is a function that uses an SDK. The generic `ExternalCall` trace is enough — no special primitives needed.

---

## Updated files

- `docs/framework.md` §1 — added scope boundary statement
- `docs/framework.md` §10 — "One Postgres database per Flux instance", added "One database per instance" and "No built-in storage" subsections
- `docs/framework.md` §18 — expanded retention section with export-before-delete pattern, configurable retention hours
- `docs/framework.md` §22 — added `flux records export/count/prune` CLI commands
