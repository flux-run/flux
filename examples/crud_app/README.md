# CRUD App

Minimal CRUD API built to test Flux with Hono, Zod, and Postgres.

## Docker

This sample now includes a containerized local setup that runs the app with Flux plus a Postgres service.

Start both services:

```sh
docker compose up --build
```

The API will be available at `http://localhost:8000` and Postgres at `localhost:5432`.

Stop the stack:

```sh
docker compose down
```

Remove the Postgres volume too:

```sh
docker compose down -v
```

## Flux path

This is the Flux entrypoint used by Docker:

```sh
flux build main_flux.ts
flux serve --skip-verify main_flux.ts
```

The Flux-specific entry uses:

- `Deno.serve(...)` for server mode
- `Deno.env.get("DATABASE_URL")` for container config
- direct SQL over the Flux `pg` shim
- `FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1` in Docker so the app can reach the local Postgres container
- schema creation is handled by Postgres init SQL, not by Flux module initialization

You can still build it manually:

```sh
cd examples/crud_app
flux build main_flux.ts
```

## Setup

Set a Postgres connection string before starting the server:

```sh
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/crud_app
```

Run the API:

```sh
deno task dev
```

That local Deno path uses `main.ts`, Drizzle, and `postgres-js` for convenience.

Or run it in containers:

```sh
docker compose up --build
```

## Replay demo

To record and replay this app through `flux-server`, start the app database, then start a Flux server against the same Postgres instance:

```sh
docker compose up -d postgres
target/debug/flux server start --database-url postgres://postgres:postgres@localhost:5432/crud_app --service-token dev-service-token
```

Serve the app with recording enabled:

```sh
export FLUX_SERVICE_TOKEN=dev-service-token
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/crud_app
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1
target/debug/flux serve --host 127.0.0.1 --port 8000 main_flux.ts
```

Create a todo and capture the execution ID from the response headers:

```sh
curl -i -X POST http://127.0.0.1:8000/todos \
  -H 'content-type: application/json' \
  -d '{"title":"Ship Flux","description":"Replay demo"}'
```

Look for the `x-flux-execution-id` header, then replay it:

```sh
target/debug/flux replay <execution_id> --url http://127.0.0.1:50051 --token dev-service-token --diff
```

The replay should return the same app response and show recorded steps for the stored Postgres checkpoints.

Build the Flux artifact:

```sh
flux build main_flux.ts
```

The Flux-served container listens on `http://localhost:8000`.

If you run the Flux path without Docker, create the table first:

```sh
psql postgres://postgres:postgres@localhost:5432/crud_app -f init.sql
```

## Endpoints

- `GET /todos`
- `GET /todos/:id`
- `POST /todos`
- `PUT /todos/:id`
- `DELETE /todos/:id`

## Example requests

Create a todo:

```sh
curl -X POST http://localhost:8000/todos \
  -H 'content-type: application/json' \
  -d '{"title":"Ship CRUD app","description":"Backed by Postgres"}'
```

Update a todo:

```sh
curl -X PUT http://localhost:8000/todos/1 \
  -H 'content-type: application/json' \
  -d '{"completed":true}'
```

Delete a todo:

```sh
curl -X DELETE http://localhost:8000/todos/1
```

## Verification

```sh
deno task check
deno task test
```