# Database Schema

This document describes the logical shape of the Flux database.

It is intentionally conceptual. The exact table names and migration details may evolve, but the product reflects these major data families.

## 1. Project And Configuration Data

This family holds:

- project identity
- runtime configuration
- secrets metadata
- route and middleware configuration
- environment and deployment settings

These tables make the runtime operable.

## 2. Function And Deployment Data

This family holds:

- function definitions
- bundle metadata
- deployment versions
- deployment history
- code version references

These tables connect running code to the debugging story.

## 3. Execution Record Data

This family holds:

- requests and triggers
- spans
- logs or structured log metadata
- status and duration summaries
- parent-child execution relationships

This is the backbone of `trace`, `errors`, and `why`.

## 4. Mutation And State History

This family holds:

- mutation records
- row-level history
- blame metadata
- before/after state when available

This is what allows Flux to answer state questions instead of only request questions.

## 5. Queue, Schedule, And Event Data

This family holds:

- jobs
- attempts and retries
- dead-letter state
- schedules and cron metadata
- event publication or delivery history

This preserves causality across async work.

## 6. Agent And Tool Data

This family holds:

- agent runs
- tool invocations
- prompts or plan metadata where relevant
- outputs and downstream execution links

This lets AI-backed work remain debuggable inside the same runtime.

## 7. Maintenance And Retention

This family holds:

- retention bookkeeping
- archival or pruning metadata
- schema versioning and migration state

These tables exist so operators can reason about what history is available and why.

## Schema Design Rule

The database schema makes the execution record queryable enough that the CLI and dashboard can explain incidents without reconstructing state from separate products.
