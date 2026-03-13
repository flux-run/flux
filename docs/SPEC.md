# Flux Product Spec

This document defines the intended 0.1 beta shape of Flux as a product and open-source project.

## Product Summary

Flux is an open-source backend runtime for teams that want a complete backend system and materially better production debugging.

Flux includes:

- functions
- HTTP routing and middleware
- database execution
- queues and schedules
- agents and tool calls
- deployments, secrets, and configuration
- an execution record with trace, replay, diff, mutation history, and `flux why`

The message should stay narrower than the feature list:

- Flux is the backend runtime for deterministic production debugging.

## Problem

When a backend fails in production, evidence is usually fragmented across:

- API logs
- function logs
- tracing systems
- queue systems
- deploy metadata
- database state

Operators spend time stitching those systems together before they can even begin causal debugging.

## Insight

This problem is not solved by adding another SDK or another observability product.

It is solved by owning enough of the execution path that the runtime can record:

- how work entered the system
- which code version handled it
- what spans and logs were produced
- what state changed
- what downstream work was created
- how the outcome differed from previous runs

## Product Goals

### 1. Make `flux why` Worth Reaching For

`flux why` should become the command people use first when something breaks.

### 2. Make Replay And Diff Credible

Replay and diff should help teams answer whether a failure is code-dependent, data-dependent, or deployment-dependent.

### 3. Make The Database Part Of Debugging

Flux should treat mutations, row history, and state blame as first-class debugging surfaces.

### 4. Ship A Complete System

Functions, gateway, database execution, async work, and agent/tool orchestration should feel like one runtime rather than several loosely connected services.

### 5. Keep Local And Production Legible

The local workflow should teach the same mental model the production system uses.

## Non-Goals

Flux is not trying to optimize first for:

- maximum configurability
- every language having perfect parity on day one
- hosted control-plane convenience
- leading with agents, workflow automation, or generic platform claims

Those things can matter, but they are not the center of the 0.1 story.

## Product Pillars

### Execution Record First

Every meaningful unit of work should leave behind an inspectable record.

### Database-Aware Runtime

The database should participate in the runtime contract so Flux can explain state changes, not just request paths.

### Complete Backend Path

The runtime should own enough of ingress, execution, state changes, and async work to preserve causality.

### Operator-Native UX

The CLI and dashboard should be optimized for investigation and incident response, not just admin forms.

## Intended User

The initial best-fit users are:

- technical founders building a serious product backend
- small platform teams who self-host and value control
- teams whose backend is Postgres-centric
- engineers who spend too much time reconstructing incidents manually

## Core Workflow

The core workflow Flux must make excellent is:

```bash
flux init
flux dev
flux function create
flux invoke --gateway
flux trace
flux why
flux incident replay
flux trace diff
```

If this loop feels exceptional, the rest of the system becomes easier to justify.

## System Scope

Flux should feel complete enough to run a real product backend:

- functions and HTTP routes
- database schema and guarded execution
- queue and retry mechanics
- schedules
- agents and tool calls
- secrets and configuration
- deployments and version history
- tracing, mutation history, replay, diff, and explanation

Completeness is important. Equal marketing weight for every subsystem is not.

## 0.1 Beta Criteria

Flux 0.1 beta is successful when:

1. a developer can start the full system locally without ceremony
2. a function can be created, invoked, traced, and explained immediately
3. deployments are visible inside the debugging story
4. one replay-plus-diff workflow is reliable enough to trust
5. queue and schedule work preserve the same record model
6. the CLI feels like one product, not a bag of commands
7. the docs explain the system clearly without private context

## Product Narrative Rule

When deciding what to build, document, or market, use this test:

> Does this make the execution record more complete, more trustworthy, or more useful during a real incident?

If the answer is no, it is probably not central to Flux.
