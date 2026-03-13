# Observability

Flux is built around an execution record, not around a pile of disconnected telemetry.

Observability in Flux helps an operator answer what happened, why it happened, and what changed.

## The Execution Record

The execution record connects:

- trigger metadata
- route and function identity
- code version and deployment
- spans and timing
- logs
- database mutations
- child jobs or downstream work
- result or failure

This is the central observability primitive of the product.

## Why Logs Alone Are Not Enough

Logs are useful, but logs by themselves cannot answer:

- which version ran
- what state changed
- whether a replay behaved differently
- what work a request triggered downstream

Flux treats logs as one part of the record, not the record itself.

## Primary Operator Surfaces

The main observability and debugging surfaces are:

- `flux tail` for live traffic
- `flux errors` for fast triage
- `flux trace` for execution structure
- `flux why` for root-cause explanation
- `flux debug` for deeper investigation
- state history and blame for mutation-level debugging
- replay, diff, and bisect for regression analysis

These tools feel connected because they are reading the same underlying record.

## Trace Model

A trace in Flux includes:

- top-level request identity
- spans across gateway, runtime, database, queue, and agent/tool work
- slow-span markers
- error spans
- timing aligned to one execution

The trace moves operators from "something failed" to "this is where it failed."

## Mutation-Aware Observability

What makes Flux different is that observability includes state changes:

- row-level mutation history
- before/after comparison when possible
- links from mutations back to requests and jobs
- deployment and commit context

This is what turns tracing into backend debugging instead of request timing alone.

## Sampling And Retention

Flux supports retention and sampling, and the defaults favor usefulness for debugging:

- enough retention to investigate real incidents
- enough fidelity to trust `why`, replay, and diff
- clear operator control over storage tradeoffs

The right question is not "do we emit telemetry?" The right question is "can we still explain a production failure later?"
