# Architecture

Flux is designed around a simple external promise:

- one runtime
- one operator surface
- one execution record model

The internal codebase keeps several subsystem boundaries, and the deployment story is a single binary serving the whole product.

## Why A Single Binary

The product is coherent for operators and developers:

- one local command starts the system
- one port exposes the product
- one CLI and dashboard talk to one runtime
- one Postgres-backed record explains what happened

The internal modules still matter, but the external experience stays unified.

## Logical Components

Flux has four main logical components:

- `runtime` - function execution and bundle loading
- `runtime` request handling - ingress, routing, validation, request IDs
- database dispatch - guarded database execution and mutation recording
- `queue` - async jobs, retries, schedules
- `api` - operator-facing APIs for deployments, traces, records, and admin actions

The `server` crate is where those pieces come together into the monolithic deployment model.

## Request Flow

The product is easiest to understand by following one request:

1. a request enters through runtime request handling
2. routing, policy, validation, and middleware are resolved
3. the runtime executes the target function
4. the function uses database dispatch for database work
5. the function may enqueue background jobs or invoke tools
6. the system writes the execution record, including spans, logs, deploy metadata, and mutations
7. the CLI and dashboard can inspect that record

The request path matters because the debugging story depends on keeping those stages linked.

## Execution Record Flow

The execution record unifies:

- ingress metadata
- code version and deployment
- spans and timing
- logs
- database mutations
- queued or scheduled follow-up work
- final response or failure

The architecture exists to make that record complete enough to trust.

## Production Topology

The production topology is:

- one externally exposed service
- one operator API namespace
- one Postgres-backed persistent state layer
- optional object or blob storage for bundles and artifacts

Internally, the server calls component modules that mirror the separate crates in this repo.

## Development Topology

The repo contains multiple crates because that keeps the system legible:

- each subsystem can be tested in isolation
- component responsibilities stay visible
- the monolith does not become an unreadable blob

Development topology does not dictate the product story.

## Why Boundaries Still Matter

Flux does not collapse everything into one indistinguishable runtime module.

The boundaries are valuable because:

- request-handling concerns are different from code execution concerns
- database execution deserves its own careful contract
- async work has different failure and retry semantics
- operator APIs stay distinct from the hot request path

The product is one system because the record is shared. The internals are still modular because the responsibilities are different.

## Open-Source Architecture Standard

A reader can answer these questions from the repo:

- where do requests enter?
- where does code execute?
- where are mutations recorded?
- where does queued work live?
- where does the debugging data come from?

If the docs make those answers obvious, the architecture is doing its job.
