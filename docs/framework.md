# Framework Overview

Flux is a runtime-first backend framework.

It is not designed as a thin library around someone else's control plane. It is designed as a complete system for running and debugging backend code with a much tighter link between execution, state, and operations.

## Product Thesis

Most backend tools optimize for writing and deploying code. Very few optimize for reconstructing what happened after production fails.

Flux is built on a different thesis:

- backend execution leaves behind a durable, queryable record
- the database is part of that record, not a separate blind spot
- async work preserves causality instead of breaking it
- operators move from alert to explanation in a few commands

The result feels less like "serverless functions plus add-ons" and more like a coherent backend runtime.

## What Flux Is

Flux is:

- a function runtime
- an in-process execution dispatcher
- a database execution contract
- a queue and scheduler
- an operator API, dashboard, and CLI
- an execution record system for debugging and replay

The system is broad on purpose. The broader runtime exists so the debugging story stays coherent.

## What Flux Is Not

Flux is not trying to be:

- a generic cloud control plane
- a loosely connected bundle of platform services
- a "functions only" product
- a logging product with a few backend helpers attached

The core value is deterministic backend debugging. Everything else exists to reinforce that.

## Product Principles

### 1. Complete System, Focused Message

The runtime includes functions, database access, queues, and schedules, but the message stays focused:

- Flux is the backend runtime for deterministic production debugging.

### 2. Local-First

Developers start the system locally, understand it from the repo, and debug it without needing a hosted control plane.

### 3. Own The Execution Path

The more of the backend path Flux owns, the more trustworthy replay, diff, mutation history, and root-cause analysis become.

### 4. Human-Usable Operations

The CLI and dashboard are useful under pressure. `trace`, `why`, replay, diff, and history matter more than endless configuration surfaces.

### 5. Architectural Clarity

The codebase preserves clean subsystem boundaries even when the deployment target is one binary.

## Intended User

Flux is best suited to teams that:

- run a Postgres-backed backend
- care about self-hosting and runtime ownership
- want a simpler local-to-production story
- lose too much time debugging incidents across multiple disconnected systems

The ideal first users are technical founders and small platform teams who feel debugging pain sharply.

## Core Loop

This loop is the center of the product:

1. initialize a project
2. start the local runtime
3. create or edit a function
4. invoke it through the full stack
5. inspect the execution record
6. understand failures with `flux why`
7. replay or diff when needed

This loop is excellent, and the complete system reinforces it.

## Architectural Shape

Flux keeps a deliberate internal shape:

- `runtime` for code execution
- dispatch-backed database execution for mutation recording
- `queue` for background work and retries
- `api` for operator-facing actions
- `server` for the single-binary deployment direction

The `runtime` records every outbound `ctx.fetch()` call — method, URL, full request and response — alongside DB mutations and function invocations. Together these form a complete, replayable execution record.

These are not arbitrary services. They are boundaries that support the execution record.

## Open Source Standard

The repo explains:

- what the product is
- why the architecture looks the way it does
- what a complete user journey looks like
- the product shape and architecture clearly enough that users immediately understand what Flux does
