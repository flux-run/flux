# Bundled Drizzle Example

Flux now supports the direct package shape that real Drizzle apps use:

```text
Drizzle -> import { Pool } from "pg" -> Flux pg layer -> Postgres
```

Official example directory: [examples/drizzle](../../examples/drizzle)

Example entries:

- [examples/drizzle/crud.ts](../../examples/drizzle/crud.ts)
- [examples/drizzle/transaction.ts](../../examples/drizzle/transaction.ts)

Install the local package graph once:

```bash
cd examples/drizzle
npm install
```

Run the CRUD proof:

```bash
cd /path/to/flowbase
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1

flux run \
  --input '{"input":{"connectionString":"postgres://user:pass@127.0.0.1:5432/app"}}' \
  examples/drizzle/crud.ts
```

Run the transaction proof:

```bash
cd /path/to/flowbase
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1

flux run \
  --input '{"input":{"connectionString":"postgres://user:pass@127.0.0.1:5432/app"}}' \
  examples/drizzle/transaction.ts
```

This is intentionally local-`node_modules` based:

- it preserves the exact package version the app installed
- it avoids esm.sh export skew
- it uses the same `pg` import shape as Node and Bun apps
- it matches the current production path better than the older adapter example