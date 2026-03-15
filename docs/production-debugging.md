# Production Debugging

Flux is built so production debugging starts from an execution record instead of from disconnected logs.

This is the incident workflow in Flux.

## 1. Triage

Start by finding the failing or slow execution:

```bash
flux errors
flux tail --errors
flux trace
```

The goal is to get to a concrete request or job ID quickly.

## 2. Inspect The Trace

Once you have an execution ID:

```bash
flux trace <request_id>
```

This answers:

- what route or function ran?
- how long did it take?
- where did time go?
- what failed first?

## 3. Ask For An Explanation

```bash
flux why <request_id>
```

`flux why` connects:

- the failure
- the relevant span or spans
- the code version
- the most relevant state changes
- the next likely debugging step

## 4. Inspect State Changes

When the problem is stateful, the next step is not more logs. It is state history:

```bash
flux state history <table> --id <primary_key>
flux state blame <table> --id <primary_key>
```

This is where Flux outperforms most backend stacks.

## 5. Replay Safely

If the incident needs reproduction:

```bash
flux incident replay --request-id <request_id>
```

Replay answers:

- does the failure still reproduce?
- was the problem tied to old code?
- was the problem tied to old state?

### Replay With Mocked Network Calls

Because every outbound `ctx.fetch()` call is recorded (method, URL, request body, response body, status), `flux incident replay` can re-run the function without hitting any external APIs. Stripe, SendGrid, Twilio, and every other integration are bypassed — their recorded responses are returned instead.

This means replay is:

- **safe** — no real charges, emails, or side effects
- **deterministic** — the same response is returned every time
- **complete** — the full execution path reproduces, not just the error

### Inspect Recorded Network Calls

To examine what external calls were made during a request:

```bash
flux trace <request_id>
```

The trace output includes network call spans in order (`call_seq`), with status codes, latencies, and error detail for failed connections.

For raw data, query `flux_internal.network_calls` directly:

```sql
SELECT call_seq, method, url, status, duration_ms, error
FROM flux_internal.network_calls
WHERE request_id = '<request_id>'
ORDER BY call_seq;
```

### Resume From Checkpoint

When a function performs multiple side effects in order — insert DB row, charge Stripe, send email — and fails partway through, the recorded calls establish a checkpoint:

1. Every completed DB mutation is in `flux_internal.state_mutations`
2. Every completed network call is in `flux_internal.network_calls`
3. The `call_seq` column establishes the exact order

An operator can inspect these tables to see exactly which steps succeeded before the failure and craft a targeted re-run that starts from the last safe point rather than re-executing from scratch.

## 6. Diff The Outcomes

Compare the original run with the replay:

```bash
flux trace diff <original_id> <replay_id>
```

The interesting output is not only timing. It is also state-level divergence.

## 7. Track Regressions

If the incident appears deploy-related:

```bash
flux bug bisect --function <name> --good <sha> --bad <sha>
```

This is where deployment metadata becomes part of the debugging product.

## What Good Incident Debugging Looks Like

A strong Flux incident workflow gives an operator:

- I find the failing execution quickly
- I see the path through the system
- I know what state changed
- I know which version ran
- I reproduce or compare behavior without rebuilding the whole world

That is the standard the product delivers.
