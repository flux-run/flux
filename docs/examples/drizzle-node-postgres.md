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

- The same OID-based parser path now applies to JSONB and one-dimensional array fields on extended queries, so custom `pg` parsers can reshape those values too.
- Date, time, timetz, timestamp, timestamptz, interval, and UUID fields now use that same parser path as exact text values, which matches the common `pg` pattern of registering app-specific parsers in JavaScript.
- BYTEA fields now surface as exact `\x...` hex strings by default, so custom `pg` parsers can convert them into byte arrays or other application-specific binary shapes.

Important compatibility note:

- Flux still does not support direct bare package imports in the runtime artifact path. This shim is the database-driver side of the integration, not the full package-loading story. It is most useful for bundled builds or for the next runtime slice that adds broader package compatibility.