# Concepts

## Core Model

Flux treats each function run as an **execution record**.

Every time `flux-runtime` handles a request, it creates one record that captures:

- input payload and output (or error)
- every checkpointed IO call (request, response, duration) in order
- total duration and HTTP status
- the entry file that served the request

All records are stored in Postgres via `flux-server`.

## Why This Matters

Debugging starts from one execution ID instead of stitching together separate log files, trace tools, and database state.

```bash
flux logs --status error          # find the failing execution
flux trace <id> --verbose         # see exactly what happened
flux why <id>                     # get a root-cause summary
```

## Checkpoints

During execution, `flux-runtime` records **checkpoint spans** at IO boundaries — for example buffered outbound HTTP calls and deterministic TCP/TLS exchanges capture the request, response, and duration.

This enables:

- deterministic replay (`flux replay`) — re-run with the same recorded responses injected
- partial continuation (`flux resume`) — continue from a specific checkpoint index
- field-level output comparison (`flux replay --diff`) — spot what changed between two runs

## Process Model

Three processes cooperate:

- `flux-server` — always-on gRPC server and Postgres store (port 50051)
- `flux-runtime` — serves user JS/TS requests and writes records to `flux-server` (port 3000)
- `flux` — short-lived CLI commands that query `flux-server`

Config (server URL + service token) is saved to `~/.flux/config.toml` after `flux init`.

## Operator Loop

1. find issue (`flux logs --status error`)
2. inspect full context (`flux trace <id> --verbose`)
3. diagnose quickly (`flux why <id>`)
4. validate fix behaviour (`flux replay <id> --diff`)
