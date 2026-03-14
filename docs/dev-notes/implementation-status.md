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
| Documentation | Active | The docs now present a coherent public product narrative. |
| Single-binary direction | Active | The `server` crate is the right architectural direction even though individual crates still exist and are still used during development. |
| CLI core loop | Active | Core loop validated end-to-end (see release gate run below). |
| Local dev story | Active | `flux dev` is the right idea and one of the most important product surfaces. |
| Gateway | Active | The request pipeline is strong and aligned with the product model. |
| Runtime | Active | Execution, bundle loading, and caching are substantive. Some endpoint and auth surfaces still need cleanup. |
| Data engine | Active | Mutation-aware execution is a core strength of the repo. |
| Queue and schedules | Shaping | Important to the complete-system story, but still need smoother execution-record integration and operator polish. |
| Replay and diff | Shaping | High value, but trustworthiness matters more than breadth here. |
| Agents | Experimental | Useful as part of the system, but not yet the headline feature. |
| WASM and multi-language parity | Experimental | Ambitious and worth keeping, but not yet a dependable flagship capability. |
| Auth and service hardening | Needs hardening | Safe defaults and service isolation need more work before broad beta testing. |

## Internal Bar

These are the implementation gates that matter most:

1. `flux init -> flux dev -> flux invoke -> flux trace -> flux why` feels clean  ✅ **Validated 2026-03-14** — full loop passes end-to-end on the monolith server
2. project and config resolution are easy to understand
3. one deployment is visibly linked to one execution record  ✅ **Validated 2026-03-14** — `flux records export` returns spans linked by `request_id`
4. one replay-plus-diff flow is believable enough to trust
5. async work preserves the same debugging model
6. defaults are safe enough for real beta users

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

