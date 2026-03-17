# Flux Product Spec (Current)

## Scope

Flux provides a backend execution runtime focused on deterministic debugging.

Its core product primitive is the execution record: input, output, and ordered checkpoints captured at Flux-owned side-effect boundaries.

The shipping surface is three binaries:

- `flux` — developer and operator CLI
- `flux-server` — gRPC server and Postgres-backed execution store
- `flux-runtime` — Deno V8 isolate executor for JS/TS entry files

## Required Operator Capabilities

- initialize once and run zero-flag (`flux init`)
- start and monitor processes (`flux server start`, `flux serve`, `flux ps`, `flux status`)
- list and filter executions (`flux logs --status --path --since --search`)
- stream live events (`flux tail`)
- inspect complete trace including top-level request/response and checkpoints (`flux trace --verbose`)
- root-cause hints (`flux why`)
- replay and compare behaviour (`flux replay --diff`)
- resume from checkpoint boundary (`flux resume --from`)
- one-off execution without long-running runtime (`flux exec`)

## Determinism Contract

Flux should preserve this invariant for deterministic replay paths:

- same code
- same input
- same recorded checkpoints at the same boundary indices

should yield the same externally observed result.

Replay and resume paths that intentionally perform live side effects are continuation flows, not pure deterministic replay, and should be documented as such.

## UX Constraints

- concise output in list views
- short IDs (8 chars) in lists, full IDs in detail views
- explicit next-step errors for connection/auth failures
- color-coded status semantics: `✓ ok` (green), `✗ error` (red), `⚠ slow` (yellow)
- after `flux init`, no repeated auth flags required

## Out of Scope (Not Yet Built)

- managed cloud deployment (`flux deploy`)
- gateway routing with per-route auth and rate limiting
- database mutation recording and row version history
- async job queues and cron schedules
- secrets management
- multi-tenant project isolation
