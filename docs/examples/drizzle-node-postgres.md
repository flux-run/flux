# Drizzle Node-Postgres Shim

Flux now exposes a small `pg`-shaped shim on top of `Flux.postgres.query(...)` for Drizzle's normal query path.

Use the included example module at [examples/flux-pg.js](../../examples/flux-pg.js):

```js
import pg from "../../examples/flux-pg.js";
import { drizzle } from "drizzle-orm/node-postgres";

const pool = new pg.Pool({
  connectionString: "postgres://user:pass@db.internal/app",
  tls: true,
  caCertPem: Deno.env.get("APP_DB_CA_PEM"),
});

const db = drizzle(pool);
```

What this shim supports today:

- `new Pool({ connectionString, tls, caCertPem })`
- `pool.query(sql, params)`
- `pool.query({ text, values, rowMode: "array" }, params)`
- `pool.end()`
- `types.builtins` and `types.getTypeParser(...)` for the basic surface Drizzle references

Current limitation:

- `pool.connect()` intentionally throws. Flux does not yet expose stateful Postgres sessions across multiple queries, so transaction-oriented APIs must fail explicitly instead of pretending to work.

Important compatibility note:

- Flux still does not support direct bare package imports in the runtime artifact path. This shim is the database-driver side of the integration, not the full package-loading story. It is most useful for bundled builds or for the next runtime slice that adds broader package compatibility.