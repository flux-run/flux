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

const client = await pool.connect();
try {
  await client.query("begin");
  await client.query({ text: "select 1", rowMode: "array" });
  await client.query("commit");
} catch (err) {
  await client.query("rollback");
  throw err;
} finally {
  await client.release();
}
```

What this shim supports today:

- `new Pool({ connectionString, tls, caCertPem })`
- `pool.query(sql, params)`
- `pool.query({ text, values, rowMode: "array" }, params)`
- `pool.connect()` returning a session-bound client
- `client.query(...)` on that connected client
- `client.release()`
- `pool.end()`
- `types.builtins`, `types.getTypeParser(...)`, and `types.setTypeParser(...)`
- query `fields` with real Postgres `dataTypeID` values for extended queries

Numeric compatibility note:

- Flux preserves `NUMERIC` values exactly by default as strings.
- If you want app-specific parsing, register a parser just like `pg`:

```js
const { types } = pg;
types.setTypeParser(types.builtins.NUMERIC, (value) => value);
```

Important compatibility note:

- Flux still does not support direct bare package imports in the runtime artifact path. This shim is the database-driver side of the integration, not the full package-loading story. It is most useful for bundled builds or for the next runtime slice that adds broader package compatibility.