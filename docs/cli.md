# CLI

The Flux CLI is the primary developer and operator interface to the runtime.

It makes the system feel simpler than it really is.

## Product Role

The CLI is not only a deployment tool. It is where the product's main promise becomes tangible:

- start the system
- ship code
- inspect executions
- explain failures
- replay incidents

If the CLI feels scattered, Flux feels scattered.

## Core Loops

### Bootstrap

```bash
flux init
flux doctor
flux dev
```

This is the local-first entry into the product.

### Build And Deploy

```bash
flux function create
flux db push
flux generate
flux deploy
flux invoke --gateway
```

This is how developers change code, evolve the database, and exercise the full stack.

### Debug And Operate

```bash
flux tail
flux errors
flux trace
flux why
flux debug
flux state history
flux incident replay
flux trace diff
flux bug bisect
```

This is the heart of the product.

## Hero Commands

The commands that matter most to Flux are:

- `flux trace`
- `flux why`
- `flux debug`
- replay and diff workflows

These are the best-designed commands in the CLI because they are the clearest expression of the product.

## Design Principles

The CLI optimizes for:

- one obvious path for a new user
- current-directory project context by default
- calm, readable output
- JSON only when requested
- next-step hints that keep users moving

The CLI does not teach outdated or cloud-first concepts as the main workflow.

## Context Model

Flux feels local-first:

- the current directory is the active project
- local development works without account setup
- remote contexts exist for deployed environments
- context switching extends the product, not redefine it

## Command Surface

It is reasonable for Flux to expose commands for:

- functions
- database and schema
- queue and schedules
- gateway config
- deployments
- traces and incidents

The command tree keeps the core debugging loop obvious.

## CLI Success Test

A new developer can:

1. start the product quickly
2. create and run a function
3. find a request
4. understand a failure
5. feel that Flux is one coherent system
