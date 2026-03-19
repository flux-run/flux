// @ts-nocheck
// Compat test: postgres.js driver (modern, fast, edge-friendly)
// Uses Flux's postgres interception layer.
import { Hono } from "npm:hono";
import postgres from "npm:postgres";

const app = new Hono();

function getSql() {
  return postgres(Deno.env.get("DATABASE_URL") ?? "postgres://localhost/postgres", {
    max: 5,
    idle_timeout: 30,
  });
}

// GET / — smoke test (no DB required)
app.get("/", (c) => c.json({ library: "postgres.js", ok: true }));

// GET /db-query — SELECT 1 to verify driver is connected
app.get("/db-query", async (c) => {
  const sql = getSql();
  try {
    const [row] = await sql`SELECT 1 AS value`;
    return c.json({ ok: true, value: row.value });
  } finally {
    await sql.end();
  }
});

// GET /db-types — verify common Postgres types are decoded correctly
app.get("/db-types", async (c) => {
  const sql = getSql();
  try {
    const [row] = await sql`
      SELECT
        42::int AS int_val,
        'flux'::text AS text_val,
        true::boolean AS bool_val,
        NOW()::timestamptz AS ts_val
    `;
    return c.json({
      ok: true,
      int_is_number: typeof row.int_val === "number",
      text_is_string: typeof row.text_val === "string",
      bool_is_bool: typeof row.bool_val === "boolean",
      ts_is_date: row.ts_val instanceof Date,
    });
  } finally {
    await sql.end();
  }
});

// POST /db-insert-select — INSERT + SELECT + DELETE
app.post("/db-insert-select", async (c) => {
  const { label } = await c.req.json();
  const sql = getSql();
  try {
    await sql`
      CREATE TABLE IF NOT EXISTS flux_pgjs_compat (
        id SERIAL PRIMARY KEY,
        label TEXT NOT NULL
      )
    `;
    const [inserted] = await sql`
      INSERT INTO flux_pgjs_compat (label) VALUES (${label}) RETURNING id, label
    `;
    const [selected] = await sql`
      SELECT id, label FROM flux_pgjs_compat WHERE id = ${inserted.id}
    `;
    await sql`DELETE FROM flux_pgjs_compat WHERE id = ${inserted.id}`;
    return c.json({ ok: true, inserted, selected });
  } finally {
    await sql.end();
  }
});

// GET /db-transaction — tagged template transaction
app.get("/db-transaction", async (c) => {
  const sql = getSql();
  try {
    const result = await sql.begin(async (sql) => {
      const [row] = await sql`SELECT NOW() AS ts`;
      return row;
    });
    return c.json({ ok: true, ts: result.ts });
  } finally {
    await sql.end();
  }
});

Deno.serve(app.fetch);
