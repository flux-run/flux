# API

The API component is the operator-facing surface of Flux.

It is where the CLI, dashboard, and automation surfaces go to inspect and manage the system.

## Responsibilities

The API owns:

- health and readiness endpoints
- deployment and version metadata
- trace and execution record queries
- mutation and state history queries
- debugging and incident workflows
- project configuration and secrets management
- operator-authenticated administrative actions

It is not the main ingress path for user traffic. That belongs to the gateway.

## Product Role

Flux has two broad request categories:

- product traffic that enters through the gateway
- operator traffic that enters through the API

The API exists so the CLI and dashboard can inspect and control the runtime without conflating operator actions with end-user request handling.

## Why The API Matters

The CLI experience depends on a coherent operator API for commands like:

- `flux trace`
- `flux why`
- `flux deploy`
- `flux state history`
- `flux incident replay`
- `flux trace diff`

If the API is inconsistent, the CLI becomes inconsistent too.

## Target Endpoint Groups

The API exposes route groups for:

- health and system status
- functions and deployments
- traces and debugging
- database and mutation inspection
- queue and schedule operations
- gateway and route configuration
- records, metrics, and operator views

See [api-reference.md](api-reference.md) for the target route map.

## Design Rule

The API is the operator surface for the whole product, not a generic CRUD layer.
