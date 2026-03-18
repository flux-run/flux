# Drizzle Node-Postgres Compatibility

Flux now exposes a real `pg` import surface for Drizzle-style apps.

Preferred app shape:

```js
import { drizzle } from "drizzle-orm/node-postgres";
import { Pool } from "pg";

const pool = new Pool({
  connectionString: "postgres://user:pass@db.internal/app",
});

const db = drizzle(pool);
```

What the compatibility layer supports today:

- `new Pool({ connectionString, tls, caCertPem })`
- `pool.query(sql, params)`
- `pool.query({ text, values, rowMode: "array" }, params)`
- `pool.connect()` returning a session-bound client
- `client.query(...)` on that connected client
- `client.release()`
- `pool.end()`
- `types.builtins`, `types.getTypeParser(...)`, and `types.setTypeParser(...)`
- query `fields` with real Postgres `dataTypeID` values for extended queries
- structured database errors including `code`, `detail`, `constraint`, `schema`, `table`, and `column`
- integer parameter binding against both `int4` and `int8` targets

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

Package-loading note:

- Flux now prefers locally installed `node_modules` packages for this path.
- The old [examples/flux-pg.js](../../examples/flux-pg.js) helper remains as a small compatibility example, but it is no longer the preferred Drizzle path.