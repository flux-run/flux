# Webhook Dedup Demo

Minimal Flux demo that shows safe webhook handling with:

- Redis as the shared-state deduplication boundary
- Postgres as the durable event log
- replay preserving the original processing result without duplicating inserts

This demo uses the same execution pattern as the idempotency example, but in a webhook context that feels closer to real production traffic.

## Execution Flow

```text
Webhook request
  ↓
REDIS GET event:key
  ↓
MISS → execute → POSTGRES INSERT → REDIS SET
HIT  → return duplicate
```

Redis answers "have we processed this event id already?". Postgres stores the durable record of processed events.

## Setup

Start Postgres and Redis locally:

```sh
docker run --rm -d --name flux-webhook-postgres \
  -e POSTGRES_USER=admin \
  -e POSTGRES_PASSWORD=password123 \
  -e POSTGRES_DB=webhook_dedup \
  -p 55433:5432 \
  postgres:17-alpine

docker run --rm -d --name flux-webhook-redis \
  -p 56380:6379 \
  redis:7-alpine
```

Initialize the table:

```sh
docker exec -i flux-webhook-postgres \
  psql -U admin -d webhook_dedup -v ON_ERROR_STOP=1 \
  < examples/webhook_dedup/init.sql
```

Start `flux-server`:

```sh
target/debug/flux server start \
  --database-url postgres://admin:password123@127.0.0.1:55433/webhook_dedup \
  --service-token dev-service-token
```

Run the demo app with recording enabled:

```sh
export FLUX_SERVICE_TOKEN=dev-service-token
export DATABASE_URL=postgres://admin:password123@127.0.0.1:55433/webhook_dedup
export REDIS_URL=redis://127.0.0.1:56380/0
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1
export FLOWBASE_ALLOW_LOOPBACK_REDIS=1

target/debug/flux run --listen --host 127.0.0.1 --port 8030 examples/webhook_dedup/main_flux.ts
```

## Demo

Send the first webhook:

```sh
curl -i -X POST http://127.0.0.1:8030/webhook \
  -H 'content-type: application/json' \
  -H 'x-event-id: evt_123' \
  -d '{"provider":"stripe","type":"invoice.paid"}'
```

Expected behavior:

- response status `202`
- header `x-webhook-status: processed`
- response contains the recorded event
- response also includes `x-flux-execution-id`

Send the exact same webhook again:

```sh
curl -i -X POST http://127.0.0.1:8030/webhook \
  -H 'content-type: application/json' \
  -H 'x-event-id: evt_123' \
  -d '{"provider":"stripe","type":"invoice.paid"}'
```

Expected behavior:

- response status `200`
- header `x-webhook-status: duplicate`
- no second Postgres insert occurs

Verify the durable state:

```sh
curl http://127.0.0.1:8030/events
```

You should still see exactly one row.

## Trace Walkthrough

First request:

```text
REDIS GET "event:evt_123" -> null
POSTGRES INSERT webhook_events -> 1 row
REDIS SET "event:evt_123" "1" -> "OK"
```

Duplicate request:

```text
REDIS GET "event:evt_123" -> "1"
```

Replay of the first request:

```text
REDIS GET -> recorded null
POSTGRES INSERT -> recorded
REDIS SET -> recorded
```

## Replay

Take the `x-flux-execution-id` from the first request and replay it:

```sh
target/debug/flux replay <execution_id> \
  --url http://127.0.0.1:50051 \
  --token dev-service-token \
  --diff
```

Replay never re-executes Redis or Postgres. It returns recorded results from checkpoints, so the original webhook handling result is preserved without duplicating the durable event insert.

## TTL

The dedup key expires automatically after one hour:

```ts
await redis.expire(seenKey, 60 * 60)
```

## Failure Scenario

If the webhook crashes after the database insert, replay resumes from recorded checkpoints and does not process the event twice.

The example also uses a Postgres unique constraint as a durable fallback, so duplicate event data is prevented even if two requests race.