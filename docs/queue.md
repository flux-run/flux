# Queue

The queue handles background work in Flux.

It exists so that async processing, retries, and scheduled work stay inside the same execution model instead of becoming a separate operational blind spot.

## Responsibilities

The queue supports:

- publishing jobs from functions or hooks
- worker execution
- retries and backoff
- dead-letter handling
- visibility into job state and history
- linkage between parent and child executions

The queue is not a side product. It is part of the same runtime story.

## Why It Matters

In many systems, requests are traceable but queued work is not.

Flux preserves causality across async boundaries so operators can answer:

- which request created this job?
- which retry failed?
- what code version processed it?
- what mutations did it make?
- did it fan out more work?

If async work leaves the execution record, the debugging story becomes incomplete.

## Retry Model

Retries are visible and explicit:

- every attempt is attributable
- timeout and backoff behavior is inspectable
- dead-letter behavior is understandable from the record

Retries are not implementation noise. They are part of the operational story.

## Schedules And Cron

Schedules fit naturally into the same model:

- a schedule trigger creates an execution
- that execution may call functions or enqueue jobs
- all follow-up work remains linked

This is why queues and schedules belong in the same product family.

## Operator Surfaces

Flux makes queue work observable through:

- queue status and depth
- per-job history
- retries and dead-letter visibility
- linked traces and mutation history

The queue feels like part of backend debugging, not a detached worker system.
