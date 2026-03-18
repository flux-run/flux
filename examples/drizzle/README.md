# Drizzle Example

This example is the official proof that unmodified Drizzle apps run on Flux.

It uses the real package imports:

```ts
import { drizzle } from "drizzle-orm/node-postgres";
import { Pool } from "pg";
```

No adapter module is required.

## Setup

Install the local example dependency graph:

```bash
cd examples/drizzle
npm install
```

The runtime resolves `drizzle-orm` from local `node_modules` and resolves `pg`
through the Flux runtime shim.

## CRUD Flow

Run the CRUD proof against a reachable Postgres instance:

```bash
cd /path/to/flowbase
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1

target/debug/flux run \
  --input '{"input":{"connectionString":"postgres://user:pass@127.0.0.1:5432/app"}}' \
  examples/drizzle/crud.ts
```

That script creates a temporary table, performs insert/select/update, then drops
the table in `finally`.

## Transaction Flow

```bash
cd /path/to/flowbase
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1

target/debug/flux run \
  --input '{"input":{"connectionString":"postgres://user:pass@127.0.0.1:5432/app"}}' \
  examples/drizzle/transaction.ts
```

That script proves transaction-bound reads and writes over the `pg` compatibility
layer.