# Runtime Architecture (Current)

Flux currently ships as three cooperating binaries:

- `flux` — CLI and operator interface (`cli/`)
- `flux-server` — gRPC server and Postgres-backed execution store (`server/` + `shared/`)
- `flux-runtime` — Deno V8 isolate that executes user JS/TS and records checkpoints (`runtime/`)

## Why Three Binaries

- CLI is short-lived commands
- server and runtime are long-running daemons
- clear ownership and easier operational debugging

## Process Model

- `flux server start` launches `flux-server` (default port 50051)
- `flux serve` launches `flux-runtime` (default port 3000, connects to `flux-server`)
- metadata files in `~/.flux/` track pid/port/entry for `flux ps` and `flux status`

## Shared Contract

A shared proto schema (`shared/proto/internal_auth.proto`) defines the gRPC payloads consumed by CLI, server, and runtime. The package is `flux.internal.v1`, service `InternalAuthService`.

## Future Direction

The goal is fully in-process communication — consolidating all subsystems into a single `flux-server` binary with no inter-process HTTP hops. The `server/` crate is the target monolith for this migration.
