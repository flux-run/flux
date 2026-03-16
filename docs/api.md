# API

Flux's operator-facing surface is a gRPC server (`flux-server`) that the CLI talks to for all commands.

## What It Handles

- auth/token validation
- execution list query (`flux logs`)
- trace fetch (`flux trace`)
- diagnosis endpoint (`flux why`)
- replay/resume operations (`flux replay`, `flux resume`)
- live event streaming (`flux tail`)
- recording new executions (written by `flux-runtime` after each request)

## Design Rule

User request traffic and operator traffic are separate:

- user requests hit `flux-runtime` HTTP endpoints (port 3000)
- operator commands hit `flux-server` gRPC endpoints (port 50051)
