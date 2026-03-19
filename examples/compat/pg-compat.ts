// @ts-nocheck
// Compat test: pg (node-postgres) driver via Flux.postgres
import { Hono } from "npm:hono";
import pg from "flux:pg";

const app = new Hono();

// GET / — smoke test (no DB required)
app.get("/", (c) => c.json({ library: "pg", ok: true }));

// GET /db-query — basic SELECT 1 to verify the driver is connected
app.get("/db-query", async (c) => {
  const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
  const result = await pool.query("SELECT 1 AS value");
  await pool.end();
  return c.json({ ok: true, value: result.rows[0]?.value });
});

// POST /db-insert-select — insert a row, select it back, delete it
app.post("/db-insert-select", async (c) => {
  const { label } = await c.req.json();
  const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });

  await pool.query(`
    CREATE TABLE IF NOT EXISTS flux_pg_compat_test (
      id SERIAL PRIMARY KEY,
      label TEXT NOT NULL,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);

  const insert = await pool.query(
    "INSERT INTO flux_pg_compat_test (label) VALUES ($1) RETURNING id, label",
    [label]
  );
  const row = insert.rows[0];

  const select = await pool.query(
    "SELECT id, label FROM flux_pg_compat_test WHERE id = $1",
    [row.id]
  );

  await pool.query("DELETE FROM flux_pg_compat_test WHERE id = $1", [row.id]);
  await pool.end();

  return c.json({ ok: true, inserted: row, selected: select.rows[0] });
});

// GET /db-transaction — commit a transaction
app.get("/db-transaction", async (c) => {
  const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
  const client = await pool.connect();

  try {
    await client.query("BEGIN");
    const result = await client.query("SELECT NOW() AS ts");
    await client.query("COMMIT");
    return c.json({ ok: true, ts: result.rows[0]?.ts });
  } catch (e) {
    await client.query("ROLLBACK");
    return c.json({ ok: false, error: String(e) }, 500);
  } finally {
    client.release();
    await pool.end();
  }
});

Deno.serve(app.fetch);
