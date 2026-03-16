# API

Flux API is the operator-facing surface used by CLI workflows.

## Primary Uses

- auth/token validation
- execution list query (`flux logs`)
- trace fetch (`flux trace`)
- diagnosis endpoint (`flux why`)
- replay/resume operations (`flux replay`, `flux resume`)
- live event streaming (`flux tail`)

## Design Rule

Operator traffic is separate from user request traffic:

- user calls hit runtime HTTP endpoints
- operator commands hit server gRPC endpoints
