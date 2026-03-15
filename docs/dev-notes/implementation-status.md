# Implementation Status

This is an internal status note for the repository.

It is not part of the public product docs set.

## Status Levels

- `Active` - the direction is correct and the core implementation exists
- `Shaping` - the product surface is clear but the implementation is still being aligned
- `Experimental` - useful for exploration, not yet a stable promise
- `Needs hardening` - the feature exists, but the operational contract is not ready

## Current Snapshot

| Area | Status | Notes |
| --- | --- | --- |
| Product narrative | Active | The strongest product direction is clear: complete runtime, debug-first story. |
| Documentation | Active | The docs now present a coherent public product narrative. Language support matrix synced across all marketing surfaces. |
| Single-binary direction | Active | The `server` crate is the right architectural direction even though individual crates still exist and are still used during development. |
| CLI core loop | Active | Core loop validated end-to-end (see release gate run below). |
| Local dev story | Active | `flux dev` is the right idea and one of the most important product surfaces. |
| Gateway | Active | The request pipeline is strong and aligned with the product model. CORS production guard added — panics at startup if `CORS_ALLOWED_ORIGINS` is unset in production. |
| Runtime | Active | Execution, bundle loading, and caching are substantive. WASM pool with dual-engine (speed/fast OptLevel) and AOT disk cache is production-worthy. |
| Data engine | Active | Mutation-aware execution is a core strength of the repo. |
| Auth and service hardening | Active | JWT + DB-stored API keys + RBAC fully implemented. Login rate-limiting (10/15min per email). Internal service token on all internal routes. `FLUX_API_KEY` fix applied 2026-03-15. |
| Queue and schedules | Shaping | Important to the complete-system story. Poller, retry, dead-letter, timeout recovery all work. `request_id` tracing fix applied 2026-03-15. |
| Replay and diff | Shaping | High value, but trustworthiness matters more than breadth here. |
| Agents | Experimental | Useful as part of the system, but not yet the headline feature. |
| WASM and multi-language parity | Experimental | 6 languages benchmarked and working (AS, Rust, Java, Go, PHP, Python). C# pending WASIP2 component executor. |

## Internal Bar

These are the implementation gates that matter most:

1. `flux init -> flux dev -> flux invoke -> flux trace -> flux why` feels clean  ✅ **Validated 2026-03-14** — full loop passes end-to-end on the monolith server
2. project and config resolution are easy to understand
3. one deployment is visibly linked to one execution record  ✅ **Validated 2026-03-14** — `flux records export` returns spans linked by `request_id`
4. one replay-plus-diff flow is believable enough to trust
5. async work preserves the same debugging model  ✅ **Queue e2e verified 2026-03-14**
6. defaults are safe enough for real beta users  ✅ **Auth hardened 2026-03-15**

## Release Gate Run — 2026-03-14

Full Phase 0 core developer loop validated:

| Step | Result | Notes |
|------|--------|-------|
| `cargo build -p server` | ✅ | All crates compile clean |
| `flux init flux-test-app` | ✅ | Scaffold created correctly |
| `flux dev` | ✅ | 76 migrations applied, server on :4000 |
| `flux function create greet` | ✅ | Scaffolded correctly |
| `flux deploy` | ✅ | 2 functions deployed (hello v1, greet v1) |
| `flux invoke hello` | ✅ | Returns `{"message":"Hello, world!"}` in 2ms |
| `flux trace <id>` | ✅ | 3-span waterfall with correct timing |
| `flux why <id>` | ✅ | State mutations + next steps rendered |
| `flux records count` | ✅ | 6 records from 2 invocations |
| `flux records export` | ✅ | JSONL with full span data |

### Bugs fixed during this run

| File | Fix |
|------|-----|
| `runtime/src/engine/pool.rs` | Cast `execution_seed: i64` → `as i32` before JSON serialisation — prevents `serde_v8` from returning a JS `BigInt` for values > 2^53, which caused `Cannot mix BigInt and other types` |
| `runtime/src/engine/bootstrap.js` | Added defensive `typeof ... === 'bigint'` guard around `execution_seed` before the XOR |
| `server/src/dispatch/api_impl.rs` | Removed stale `tenant_id` / `project_id` columns from `INSERT INTO platform_logs` — dropped by migration `20260314000042`; the silent failure was the reason `flux records count` always returned 0 |
| `schemas/api/20260312000029_route_notify_trigger.sql` | `ON routes` → `ON flux.routes` (table moved to flux schema in `...028`) |
| `schemas/api/20260313000035_routes.sql` | Re-attach `route_change_notify` trigger after `DROP TABLE ... CASCADE` |
| `schemas/api/20260314000040_drop_s3_storage.sql` | `ALTER TABLE deployments` → `flux.deployments` |
| `schemas/api/20260314000041_fs_bundles.sql` | Same schema-qualification fix |
| `schemas/api/20260315000045_queue_bindings.sql` | `REFERENCES functions(id)` → `flux.functions(id)` |
| `cli/src/dev.rs` | Added `QUEUE_MIGRATIONS` static and `queue_m.run()` call — queue migrations were not being applied, causing `relation "jobs" does not exist` |

## Audit Run — 2026-03-15

Comprehensive audit of all files changed since the 2026-03-14 gate run.

### Bugs found and fixed

| File | Bug | Fix |
|------|-----|-----|
| `api/src/middleware/auth.rs` | **Security/correctness:** `FLUX_API_KEY` path fell through without injecting `RequestContext` or calling `next.run()` — a valid static API key returned 401. Every production deployment using `FLUX_API_KEY` was broken. | Add `req.extensions_mut().insert(RequestContext); return next.run(req).await;` after the constant-time match succeeds. |
| `queue/src/worker/executor.rs` | **Runtime:** `UPDATE flux.jobs SET started_at …` used the wrong schema. The `jobs` table is in the `public` schema (all migrations and code use unqualified `jobs`). The query silently failed — `started_at` / `request_id` were never stamped, breaking `flux trace <id>` for async jobs. | Change `flux.jobs` → `jobs`. |
| `runtime/Cargo.toml` | **Build:** `wasmtime-wasi = { features = ["async"] }` — the `async` feature was removed in wasmtime v28 (async is now built-in). Build failure on `cargo build -p server`. | Remove `features = ["async"]` from wasmtime-wasi dependency. |

### Areas reviewed — no issues found

- `api/src/auth/routes.rs` — login rate-limiting (10/15min per email) looks correct
- `gateway/src/router.rs` — production CORS guard is correct (panics if env is empty in production)
- `gateway/src/handlers/dispatch.rs` — request pipeline steps 1–8 are correct
- `gateway/src/metrics.rs` — fire-and-forget `gateway_metrics` insert correct (search_path=flux,public resolves unqualified table correctly)
- `queue/src/worker/poller.rs` — graceful shutdown drain logic is correct
- `queue/src/services/retry_service.rs` — unqualified `jobs` is correct for public schema
- `runtime/src/engine/wasm_executor.rs` — AOT compile, fuel-based limits, WASI argv embedding all look correct
- `runtime/src/engine/wasm_pool.rs` — dual-engine (per-module OptLevel), LRU cache, disk cache, semaphore all look correct
- `runtime/src/engine/pool.rs` — backpressure guard, affinity routing, isolate pool sizing all look correct
- `cli/src/dev.rs` — embedded Postgres, hot-reload watcher, graceful shutdown all correct
- `cli/src/trace.rs` / `cli/src/why.rs` / `cli/src/doctor.rs` — display-only, no logical issues
- `data-engine/src/executor/db_executor.rs` — SET LOCAL search_path correct, mutation logging atomic with data write
- `api/src/secrets/service.rs` — AES-256-GCM encrypt/decrypt, no secrets in logs

