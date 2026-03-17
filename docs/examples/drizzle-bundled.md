# Bundled Drizzle Example

This example keeps Drizzle on the correct seam for Flux v1:

```text
Drizzle -> pg-compatible Flux shim -> Rust ops -> Postgres
```

Example entry: [examples/drizzle-basic.ts](../../examples/drizzle-basic.ts)

```ts
import { drizzle } from "npm:drizzle-orm/node-postgres";
import { sql } from "npm:drizzle-orm";
import pg from "./flux-pg.js";

export default async function handler({ input }) {
  const pool = new pg.Pool({
    connectionString: String(input.connectionString),
  });

  const db = drizzle(pool);

  try {
    const result = await db.execute(
      sql`select ${String(input.name ?? "flux")}::text as name`,
    );

    return {
      rows: result.rows,
    };
  } finally {
    await pool.end();
  }
}
```

Build it:

```bash
flux build examples/drizzle-basic.ts
```

This example is intentionally handler-shaped instead of server-shaped:

- it keeps the database seam explicit
- it avoids relying on unsupported runtime env access
- it proves the `npm:drizzle-orm/node-postgres` import path can be bundled into a Flux artifact

Use [examples/flux-pg.js](../../examples/flux-pg.js) as the blessed adapter until the runtime exposes a more polished first-party package surface.