# Concepts

Flux has multiple subsystems, but the product only works if they all reinforce one mental model.

## Execution Record

The execution record is the core primitive of Flux.

An execution record connects:

- the trigger that started work
- the function, job, or agent step that ran
- the code version that handled it
- spans and logs
- database reads and mutations
- downstream jobs or follow-up triggers
- the final result

If a feature does not strengthen the execution record, it is not central to Flux.

## Function

A function is the smallest deployable unit of application logic.

Functions can be triggered by:

- an HTTP route
- a queue worker
- a scheduled job
- an event
- an agent tool call

Flux functions matter because they live inside one runtime and one debugging model.

## Gateway

The gateway is the controlled entrypoint into the system.

It gives Flux:

- stable routing
- auth and policy enforcement
- request validation
- middleware hooks
- one top-level request ID and trace root

The gateway is where an execution becomes a record.

## Data Engine

The data engine is the database layer that Flux can reason about.

It exists so that:

- database access is part of the runtime contract
- mutations can be attributed to a specific execution
- row history, blame, replay, and diff become possible

The data engine is what prevents the database from becoming a debugging blind spot.

## Queue

The queue is how Flux handles async work without losing causal context.

The queue preserves:

- parent-child links between executions
- retry history
- timeout and dead-letter behavior
- mutation attribution

Async work is not outside the product. It is part of the same record model.

## Schedule

A schedule is just a time-based trigger into the same runtime.

Cron exists in Flux so that scheduled work uses the same:

- execution record
- retry model
- tracing
- code versioning
- mutation history

## Agent

Agents are another execution surface, not a separate product category.

In Flux, an agent is debuggable as a backend execution:

- prompts and tool calls are inspectable
- external calls are traced
- state changes are attributable
- follow-up work stays linked

## Deployment

Deployments are part of the causal graph.

Useful backend debugging always asks:

- what version ran?
- what changed since the last good execution?
- did the incident start after a deploy?

That is why deployments belong inside the product rather than in a separate CI system narrative.

## Replay

Replay is controlled re-execution of a past request or incident.

The value of replay is not "run it again." The value is:

- reproduce a failure safely
- compare old versus new behavior
- separate code problems from data problems
- inspect what changed at the state level

Replay is credible because the runtime owns enough of the execution path.

## `flux why`

`flux why` is the product thesis in one command.

It answers:

- what failed?
- where did the failure start?
- what changed?
- what state did it mutate?
- what does the operator do next?

`flux why` is a command people reach for before logs. That is the center of gravity.

## Complete System, Focused Story

Flux includes functions, gateway, database execution, queue, schedules, agents, secrets, and deployment because the execution record has to span the whole backend.

But the product message stays narrow:

- Flux is the backend runtime for deterministic production debugging.
- The complete system exists to make that statement true.
