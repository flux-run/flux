# Idempotency Demo

Flux guarantees idempotent outcomes across distributed requests. The same logical request converges to one durable order, even across retries, crashes, and replay, while staying fully observable through checkpoints and trace.

Minimal Flux demo that shows idempotent request handling with:

- Redis as the shared-state boundary
- Postgres as the durable side-effect boundary
- replay preserving the original result without duplicating inserts

This example uses the native `Flux.redis.createClient(...)` primitive so it bundles cleanly through `flux build` today. The runtime also supports the compatibility surface `import { createClient } from "redis"`, but the point of this demo is the execution model, not the import style.

## Execution Flow

```text
Request
  ↓
REDIS GET idempotency:key
  ↓
MISS → execute → POSTGRES INSERT → REDIS SET
HIT  → return stored result
```

The core flow is:

1. read `idempotency:<key>` from Redis
2. if present, return the stored response envelope
3. otherwise insert the order into Postgres
4. store the canonical response in Redis with a TTL
5. return the created order

Redis coordinates the idempotency key. Postgres is the durable source of truth for orders.

## Setup

Start Postgres and Redis locally:

```sh
docker run --rm -d --name flux-idempotency-postgres \
  -e POSTGRES_USER=admin \
  -e POSTGRES_PASSWORD=password123 \
  -e POSTGRES_DB=idempotency_demo \
  -p 55432:5432 \
  postgres:17-alpine

docker run --rm -d --name flux-idempotency-redis \
  -p 56379:6379 \
  redis:7-alpine
```

Initialize the table:

```sh
docker exec -i flux-idempotency-postgres \
  psql -U admin -d idempotency_demo -v ON_ERROR_STOP=1 \
  < examples/idempotency/init.sql
```

Start `flux-server`:

```sh
target/debug/flux server start \
  --database-url postgres://admin:password123@127.0.0.1:55432/idempotency_demo \
  --service-token dev-service-token
```

Run the demo app with recording enabled:

```sh
export FLUX_SERVICE_TOKEN=dev-service-token
export DATABASE_URL=postgres://admin:password123@127.0.0.1:55432/idempotency_demo
export REDIS_URL=redis://127.0.0.1:56379/0
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1
export FLOWBASE_ALLOW_LOOPBACK_REDIS=1

target/debug/flux run --listen --host 127.0.0.1 --port 8020 examples/idempotency/main_flux.ts
```

## Demo

Send the first request:

```sh
curl -i -X POST http://127.0.0.1:8020/orders \
  -H 'content-type: application/json' \
  -H 'idempotency-key: order-123' \
  -d '{"sku":"flux-shirt","quantity":1}'
```

Expected behavior:

- response status `201`
- header `x-idempotency-status: created`
- response contains the created order
- response also includes `x-flux-execution-id`

Send the exact same request again:

```sh
curl -i -X POST http://127.0.0.1:8020/orders \
  -H 'content-type: application/json' \
  -H 'idempotency-key: order-123' \
  -d '{"sku":"flux-shirt","quantity":1}'
```

Expected behavior:

- response status still `201`
- header `x-idempotency-status: replayed`
- response body is identical to the first request
- no second Postgres insert occurs

Verify the durable state:

```sh
curl http://127.0.0.1:8020/orders
```

You should still see exactly one row.

## Replay

Take the `x-flux-execution-id` from the first request and replay it:

```sh
target/debug/flux replay <execution_id> \
  --url http://127.0.0.1:50051 \
  --token dev-service-token \
  --diff
```

The replay should:

- return the same JSON response
- preserve the original `201` result
- show recorded Redis and Postgres steps
- avoid duplicating the order insert

Replay never re-executes Redis or Postgres. It returns recorded checkpoint results.

## Trace Walkthrough

First request:

```text
REDIS GET "idempotency:order-123" -> null
POSTGRES INSERT idempotent_orders -> 1 row
REDIS SET "idempotency:order-123" "{...}" -> "OK"
```

Duplicate request:

```text
REDIS GET "idempotency:order-123" -> "{...}"
```

Replay of the first request:

```text
REDIS GET -> recorded null
POSTGRES INSERT -> recorded
REDIS SET -> recorded
```

This is the key difference: Flux shows exactly why the duplicate request is skipped and why replay does not create a second order.

## Failure Scenario

If the request crashes after the database write but before Flux records the execution, the first attempt may have no replayable history at all.

In that case, correctness comes from retry convergence rather than replay completeness:

- Redis still misses because the key was never written
- Postgres unique enforcement prevents a second durable order
- the retry reconstructs the canonical response from durable truth and then writes Redis

The example also uses a Postgres unique constraint as a durable fallback. Redis coordinates the fast path; if requests race past coordination, Postgres still prevents duplicate durable data.

## TTL

Idempotency keys expire automatically after one hour:

```ts
await redis.expire(redisKey, 60 * 60)
```

## What this proves

This demo is not just "Redis support".

It demonstrates that Flux can guarantee idempotent execution with:

- shared state across isolates
- durable convergence to one logical effect
- replay-safe request handling
- traceable boundary behavior