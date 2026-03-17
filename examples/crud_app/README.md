# CRUD App

Minimal Deno CRUD API built with Hono, Drizzle ORM, Zod, and Postgres.

## Docker

This sample now includes a containerized local setup for the app plus Postgres.

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

## Flux status

This sample already builds as a Flux artifact:

```sh
cd examples/crud_app
flux build main.ts
```

That validates the bundled module graph for Hono + Drizzle.

Current runtime caveat:

- the sample still reads `DATABASE_URL` via `Deno.env.get(...)`
- the sample still uses the `drizzle-orm/postgres-js` driver for local Deno development

Those choices are fine for local Deno usage, but Flux's proven database seam today is the bundled artifact path plus the `pg`-compatible Flux shim.

## Setup

Set a Postgres connection string before starting the server:

```sh
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/crud_app
```

Run the API:

```sh
deno task dev
```

Or run it in containers:

```sh
docker compose up --build
```

Build the Flux artifact:

```sh
flux build main.ts
```

The server starts on `http://localhost:8000` by default. Set `PORT` to override it.

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