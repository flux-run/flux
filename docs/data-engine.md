# Data Engine

The data engine is the database execution layer inside Flux.

It exists because request tracing alone is not enough. Backend debugging only becomes truly useful when state changes are attributable to specific executions.

## Responsibilities

The data engine owns:

- guarded query and mutation execution
- policy enforcement around database access
- transaction handling
- mutation logging
- hook and event dispatch tied to database work
- query cost and timeout protections

It is not just a SQL proxy. It is the part of Flux that makes the database visible to the execution record.

## Why It Matters

Without a mutation-aware data layer, most incidents still end with:

- manual SQL inspection
- guesswork about which request changed a row
- weak replay and diff workflows
- poor visibility into state-driven regressions

The data engine is what allows Flux to answer state-level questions such as:

- who changed this row?
- what request caused this mutation?
- what did the row look like before and after?
- did the replay produce the same state changes?

## Mutation Recording

Mutation recording is one of the most important product capabilities.

A useful mutation record links:

- table and primary key
- before and after state where possible
- request or job identity
- code version
- timestamp
- actor or auth context where relevant

This is the foundation for history, blame, replay, and diff.

## Query Guarding

The data engine also protects the system from accidental bad queries by applying:

- cost or complexity limits
- timeouts
- policy checks
- controlled search path or schema routing

The product benefit is not only safety. It is that the runtime can explain why a query failed or was denied.

## Hooks, Events, And Side Effects

The data engine is also the natural place to connect:

- post-mutation hooks
- domain events
- queued follow-up work

This matters because state changes often create more work, and Flux preserves those causal links.

## Product Role

The data engine is one of the strongest reasons Flux is more than a function runner.

It is the bridge between request debugging and state debugging.
