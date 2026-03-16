# Flux Product Spec (Current)

## Scope

Flux provides a backend execution runtime focused on deterministic debugging.

## Shipping Surface

- CLI: `flux`
- Server process: `flux-server`
- Runtime process: `flux-runtime`
- Postgres-backed execution and checkpoint storage

## Required Operator Capabilities

- initialize once and run zero-flag (`flux init`)
- list/filter executions (`flux logs`)
- inspect complete trace including top-level request/response (`flux trace --verbose`)
- root-cause hints (`flux why`)
- replay and compare behavior (`flux replay --diff`)
- process and stack health (`flux ps`, `flux status`)
- one-off execution without long-running runtime (`flux exec`)

## UX Constraints

- concise output in list views
- short IDs in lists, full IDs in detail
- explicit next-step errors for connection/auth failures
- color-coded status semantics (`ok/error/slow`)
