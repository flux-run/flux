# DB Then Remote

Small v1 proof app for Flux:

- write a row to Postgres
- call an external HTTP system
- leave the external system off to force failure
- turn it on and use replay or resume

## Files

- `main_flux.ts`: Flux listener app
- `remote_system.js`: standalone mock external system
- `init.sql`: schema for the app table

## Schema

The app writes to `outbound_dispatches`.

`status` starts as `pending`.
After the remote call succeeds, the app updates the row to `delivered`.

## Setup

Create a database and apply the schema:

```sh
createdb db_then_remote
psql postgres://postgres:postgres@localhost:5432/db_then_remote -f init.sql
```

Start Flux server against the same database:

```sh
target/debug/flux server start \
  --database-url postgres://postgres:postgres@localhost:5432/db_then_remote \
  --service-token dev-service-token
```

Start the app:

```sh
export FLUX_SERVICE_TOKEN=dev-service-token
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/db_then_remote
export FLUXBASE_ALLOW_LOOPBACK_POSTGRES=1
export FLUXBASE_ALLOW_LOOPBACK_FETCH=1
export REMOTE_BASE_URL=http://127.0.0.1:9010

target/debug/flux run --listen --host 127.0.0.1 --port 8010 main_flux.ts
```

`FLUXBASE_ALLOW_LOOPBACK_FETCH=1` is only needed for local development when the
mock remote system is bound to `127.0.0.1`.

## Success path

Start the external system:

```sh
node remote_system.js
```

Create a dispatch:

```sh
curl -i -X POST http://127.0.0.1:8010/dispatches \
  -H 'content-type: application/json' \
  -d '{"orderId":"order-123","message":"ship it"}'
```

Expected result:

- HTTP `201`
- `x-flux-execution-id` header present
- row status becomes `delivered`

## Failure path

Leave the external system off and send the same request:

```sh
curl -i -X POST http://127.0.0.1:8010/dispatches \
  -H 'content-type: application/json' \
  -d '{"orderId":"order-456","message":"retry me"}'
```

Expected result:

- request fails
- execution is still recorded
- Postgres insert is checkpointed
- row stays `pending`

Inspect it:

```sh
target/debug/flux trace <execution_id>
```

## Replay and resume

Start the external system after the failure:

```sh
node remote_system.js
```

Then inspect or continue the failed execution:

```sh
target/debug/flux replay <execution_id> --diff
target/debug/flux resume <execution_id>
```

The target behavior for this example is:

- replay shows the recorded DB checkpoint and the external boundary
- resume avoids duplicating the already-recorded DB write
- once the remote system is reachable, the execution can complete

## Current verified behavior

With the current runtime:

- remote off: the request fails after the insert and leaves the row as `pending`
- remote on with `FLUXBASE_ALLOW_LOOPBACK_FETCH=1`: the request succeeds and the row becomes `delivered`
- replay of the failed execution preserves the original `500` response
- resume of the failed execution currently returns from the recorded Postgres checkpoint without completing the remote call

That last point is the current gap this example is meant to expose.