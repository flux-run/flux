# API Reference (Operator RPC)

Package: `flux.internal.v1`

Service: `InternalAuthService`

## RPCs

- `ValidateToken` — validate service token/auth mode
- `ListLogs` — list execution rows for `flux logs`
- `RecordExecution` — runtime write path for execution + checkpoints
- `GetTrace` — fetch full trace for `flux trace`
- `Why` — diagnosis hints for `flux why`
- `Tail` — stream live events for `flux tail`
- `Replay` — replay execution with checkpoint injection
- `Resume` — resume from checkpoint boundary

Proto source lives in `shared/proto/internal_auth.proto`.
