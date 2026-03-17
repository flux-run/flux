# API Reference (Operator RPC)

Package: `flux.internal.v1`

Service: `InternalAuthService`

Proto source: `shared/proto/internal_auth.proto`

## RPCs

| RPC | Used by | Purpose |
|-----|---------|--------|
| `ValidateToken` | `flux init`, `flux auth`, `flux run --listen` | Validate service token and return auth mode |
| `ListLogs` | `flux logs` | List execution rows with optional filter |
| `RecordExecution` | `flux-runtime` | Write execution record, checkpoints, and console logs after each request |
| `GetTrace` | `flux trace` | Fetch full trace including request/response, console logs, and checkpoint spans |
| `Why` | `flux why` | Return root-cause diagnosis for an execution |
| `Tail` | `flux tail` | Stream live execution events |
| `Replay` | `flux replay` | Re-run execution with recorded checkpoints injected |
| `Resume` | `flux resume` | Continue execution from a checkpoint boundary |
