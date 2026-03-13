# Flux

Flux is an open-source backend runtime where every execution is a record.

It combines functions, HTTP routing, database access, queues, schedules, agents, secrets, and a debugging CLI into one system. The product goal is not "more backend features." The product goal is to make production debugging deterministic because Flux owns the execution path.

> Docs note: this repository is still in development. The docs in this repo describe the intended 0.1 beta product shape and the direction the codebase is converging toward.

## Why Flux Exists

Modern backends scatter evidence across logs, traces, queues, deploy history, and database state. When something breaks, teams spend hours reconstructing what happened.

Flux is built around one idea:

- every request, job, schedule trigger, and agent step should produce one execution record
- that record should connect code version, spans, logs, database mutations, queued work, and outcomes
- debugging should start from the record, not from guesswork

That is why commands like `flux trace`, `flux why`, replay, diff, mutation history, and bisect sit at the center of the product.

## What Flux Includes

Flux is designed as a complete backend runtime:

- functions for synchronous application logic
- a gateway for routing, auth, validation, and middleware
- a data engine for guarded database access and mutation recording
- queues and schedules for background work
- agents and tool execution for AI-backed workflows
- secrets, deployment, and project configuration
- a CLI built around setup, deployment, and incident debugging

The completeness matters because the debugging model only works if Flux can see the whole execution path.

## Core Developer Loop

The ideal developer loop looks like this:

```bash
flux init my-app
cd my-app
flux dev
flux function create create_user
flux invoke create_user --gateway --payload '{"email":"user@example.com"}'
flux trace
flux why <request_id>
```

The target experience is:

- one project
- one runtime
- one local command to start everything
- one place to inspect what happened

## Product Positioning

Flux should feel like:

- a complete backend system
- self-hosted and owned by the team using it
- strongly opinionated about execution records and deterministic debugging

Flux should not feel like:

- a generic cloud control plane
- a bundle of unrelated platform features
- "just another serverless framework"

The headline is debugging. The rest of the system is proof that debugging can stay coherent across the whole backend.

## Architecture At A Glance

Flux has a target production topology and a practical repository topology.

Target production topology:

- one binary
- one port
- one Postgres-backed execution record
- one operator surface for CLI and dashboard

Repository topology:

- separate Rust crates keep boundaries clear while the product converges
- the `server` crate represents the monolithic deployment direction
- `gateway`, `runtime`, `data-engine`, `queue`, and `api` can still be developed independently

See [docs/single-binary-architecture.md](docs/single-binary-architecture.md) for the full architecture narrative.

## Repository Map

- `cli/` - developer and operator CLI
- `server/` - monolithic runtime entrypoint
- `gateway/` - ingress, routing, auth, validation, middleware
- `runtime/` - function execution and bundle loading
- `data-engine/` - database execution, mutation logging, hooks, policies
- `queue/` - async jobs, retries, worker execution
- `api/` - operator-facing APIs for deployments, traces, records, admin actions
- `agent/` - agent execution primitives
- `dashboard/` - internal/product dashboard UI
- `frontend/` - marketing site and public docs UI
- `scaffolds/` - project and function templates used by `flux init` and `flux function create`
- `docs/` - product, architecture, and component documentation

## Start Here

- [docs/README.md](docs/README.md) - documentation map
- [docs/quickstart.md](docs/quickstart.md) - first-run flow
- [docs/concepts.md](docs/concepts.md) - core product model
- [docs/cli.md](docs/cli.md) - command-line workflows
- [docs/production-debugging.md](docs/production-debugging.md) - incident workflow
- [docs/SPEC.md](docs/SPEC.md) - product goals and design constraints

## Open Source Direction

Flux is intended to be an open-source backend runtime for teams that want:

- full control over runtime and data
- a simpler local-to-production mental model
- much stronger operational debugging than logs-first stacks provide

The project should be understandable from the repo itself. The docs aim to explain both the intended product and the architecture that makes that product possible.
