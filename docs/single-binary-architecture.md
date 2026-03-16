# Runtime Architecture (Current)

Despite this filename, current Flux runtime is intentionally three binaries:

- `flux` — CLI and operator interface
- `flux-server` — gRPC + persistence to Postgres
- `flux-runtime` — executes user JS and records checkpoints

## Why Three Binaries

- CLI is short-lived commands
- server/runtime are long-running daemons
- clear ownership and easier operational debugging

## Process Model

- `flux server start` launches `flux-server`
- `flux serve` launches `flux-runtime`
- metadata files in `~/.flux` track pid/port/entry for status UX

## Shared Contract

A shared proto schema defines RPC payloads consumed by CLI, server, and runtime.
