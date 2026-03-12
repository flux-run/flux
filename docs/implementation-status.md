# Implementation Status

**Date:** March 12, 2026
**Source of truth:** [framework.md §24](framework.md#24-implementation-phases)

---

## Phase summary

| Phase | Focus | Status | Estimate |
|---|---|---|---|
| **Phase 0** | Prove the debugging magic | Not started | 2–4 weeks |
| **Phase 1** | Developer experience | Not started | — |
| **Phase 2** | Type safety & database | Not started | — |
| **Phase 3** | Production readiness | Not started | — |
| **Phase 4+** | WASM, advanced features | Not started | — |

---

## Phase 0 — Prove the debugging magic

Smallest version that validates the core value proposition end-to-end.

**Scope:**
```
flux init     → scaffold project with flux.toml + functions/
flux dev      → starts all services locally (orchestrator + embedded Postgres)
flux invoke   → call a function via gateway
flux trace    → show execution record
flux why      → root cause from execution record
```

**What needs building:**

| Component | Work | Estimate |
|---|---|---|
| `cli/src/dev.rs` | Process orchestrator: spawn 5 services, combined logs, graceful shutdown, health checks | ~300 lines |
| `flux.toml` parser | TOML parser in CLI, `flux init` writes it | ~100 lines |
| Gateway `LOCAL_MODE` | Skip tenant resolution, accept all requests | ~50 lines |
| Embedded Postgres | Auto-start, data at `.flux/pgdata/`, port assignment | ~200 lines |
| `flux trace` CLI | Query execution records, format output | ~150 lines |
| `flux why` CLI | Parse execution record, pattern-match root cause | ~200 lines |

**What already exists in the Rust codebase:**

| Component | Status | Location |
|---|---|---|
| CLI framework | ✅ 50+ commands | `cli/src/` |
| Gateway routing | ✅ Production-grade | `gateway/src/` |
| Runtime (Deno V8) | ✅ Isolate pool + execution | `runtime/src/` |
| Data Engine | ✅ Query compiler, hooks, events | `data-engine/src/` |
| Queue | ✅ Worker pool, retries, dead letter | `queue/src/` |
| API | ✅ 19 route modules | `api/src/` |
| Span recording | ✅ `platform_logs` table | `gateway/src/routes/proxy.rs` |
| Mutation recording | ✅ `state_mutations` table | `data-engine/src/` |
| Request envelope | ✅ `trace_requests` table | `gateway/src/routes/proxy.rs` |

The recording infrastructure exists. The work is wiring it into a coherent
`flux dev` experience and finishing CLI output formatting.

---

## What Phase 0 proves

- Execution recording works automatically (no user code changes)
- `flux why` genuinely saves debugging time
- Developers want Flux for the debugging alone

---

## Existing CLI commands (implemented)

| Category | Commands |
|---|---|
| Auth | `flux login`, `flux logout`, `flux auth status` |
| Project | `flux init`, `flux new` |
| Functions | `flux function create/list/delete`, `flux invoke`, `flux deploy` |
| Secrets | `flux secrets set/get/list/delete` |
| Tools | `flux tools list`, `flux tools connected` |
| Gateway | `flux route create/list/delete/activate` |

---

## Not yet implemented (by phase)

| Phase | Commands |
|---|---|
| 0 | `flux dev`, `flux trace`, `flux why` |
| 1 | Hot reload, `flux build`, `flux deploy --target local` |
| 2 | `flux generate`, `flux db push/diff/migrate` |
| 3 | `flux test`, `flux add <tool>`, `defineMiddleware()` |
| 4+ | WASM support, `flux incident replay`, `flux bug bisect` |

---

*For the full phase breakdown, see
[framework.md §24](framework.md#24-implementation-phases).*
