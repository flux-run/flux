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
- spans across request handling, runtime, database, and queue
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

## Network Call Recording

Every outbound HTTP request made through `ctx.fetch()` is recorded alongside the DB mutations and function invocations in the same execution record.

Each recorded network call captures:

- `method` and `url` for routing context
- `request_headers` and `request_body` (the exact payload sent)
- `status`, `response_headers`, and `response_body` (the exact response received)
- `duration_ms` for latency attribution
- `error` when the connection failed before a response arrived
- `call_seq` — a monotonic counter scoped to the request, so calls can be replayed in order

Records are written to `flux_internal.network_calls` atomically and asynchronously (fire-and-forget via `spawn_local`) so they do not add latency to the live path.

### Why Network Call Recording Matters

Recording the outbound surface of a function enables two things that logs alone cannot:

**Full replay with mocked responses.** Because the response body is stored, `flux incident replay` can re-run the exact same function and return the same data from every external API call — without hitting Stripe, SendGrid, or any other third party. The replay is deterministic.

**Resume from checkpoint.** For a function such as a payment handler:

1. Insert order row → recorded as a DB mutation
2. Charge Stripe → recorded as a network call (status 200, charge ID in response body)
3. Send confirmation email → network call fails

On re-run, Flux knows step 2 already succeeded with a specific charge ID. The operator can inspect `flux_internal.network_calls` for that `request_id` and see exactly where execution stopped, which response was last received, and which side effects already committed. Replay can be configured to skip already-succeeded calls or return their cached response.

## Sampling And Retention

Flux supports retention and sampling, and the defaults favor usefulness for debugging:

- enough retention to investigate real incidents
- enough fidelity to trust `why`, replay, and diff
- clear operator control over storage tradeoffs

The right question is not "do we emit telemetry?" The right question is "can we explain a production failure later?"
