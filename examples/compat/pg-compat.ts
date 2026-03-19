// @ts-nocheck
// Compat test: pg (node-postgres) driver via Flux.postgres
import { Hono } from "npm:hono";
import pg from "flux:pg";

const app = new Hono();

function getPool() {
  return new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
}

// GET / — smoke test (no DB required)
app.get("/", (c) => c.json({ library: "pg", ok: true }));

// GET /db-query — basic SELECT 1
app.get("/db-query", async (c) => {
  const pool = getPool();
  const result = await pool.query("SELECT 1 AS value");
  await pool.end();
  return c.json({ ok: true, value: result.rows[0]?.value });
});

// POST /db-insert-select — INSERT + SELECT + DELETE
app.post("/db-insert-select", async (c) => {
  const { label } = await c.req.json();
  const pool = getPool();
  await pool.query(`
    CREATE TABLE IF NOT EXISTS flux_pg_compat_test (
      id SERIAL PRIMARY KEY,
      label TEXT NOT NULL,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  const insert = await pool.query(
    "INSERT INTO flux_pg_compat_test (label) VALUES ($1) RETURNING id, label",
    [label],
  );
  const row = insert.rows[0];
  const select = await pool.query(
    "SELECT id, label FROM flux_pg_compat_test WHERE id = $1",
    [row.id],
  );
  await pool.query("DELETE FROM flux_pg_compat_test WHERE id = $1", [row.id]);
  await pool.end();
  return c.json({ ok: true, inserted: row, selected: select.rows[0] });
});

// GET /db-transaction — successful transaction
app.get("/db-transaction", async (c) => {
  const pool = getPool();
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

// ── Failure cases ──────────────────────────────────────────────────────────

// GET /db-rollback — explicit ROLLBACK, then verify data not persisted
app.get("/db-rollback", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query(`
      CREATE TABLE IF NOT EXISTS flux_pg_rollback_test (
        id SERIAL PRIMARY KEY, val TEXT
      )
    `);
    await client.query("BEGIN");
    await client.query("INSERT INTO flux_pg_rollback_test (val) VALUES ($1)", ["should-rollback"]);
    await client.query("ROLLBACK");
    const check = await client.query("SELECT COUNT(*) AS cnt FROM flux_pg_rollback_test");
    return c.json({ ok: true, rows_after_rollback: Number(check.rows[0]?.cnt) });
  } finally {
    client.release();
    await pool.end();
  }
});

// POST /db-constraint — unique constraint violation caught + returned gracefully
app.post("/db-constraint", async (c) => {
  const pool = getPool();
  try {
    await pool.query(`
      CREATE TABLE IF NOT EXISTS flux_pg_unique_test (
        id SERIAL PRIMARY KEY,
        uniq_key TEXT UNIQUE NOT NULL
      )
    `);
    await pool.query("INSERT INTO flux_pg_unique_test (uniq_key) VALUES ($1)", ["flux-key"]);
    try {
      await pool.query("INSERT INTO flux_pg_unique_test (uniq_key) VALUES ($1)", ["flux-key"]);
      return c.json({ ok: false, error: "expected constraint violation but none thrown" }, 500);
    } catch (e: any) {
      return c.json({ ok: true, caught: true, code: e?.code });
    }
  } finally {
    await pool.query("DROP TABLE IF EXISTS flux_pg_unique_test").catch(() => {});
    await pool.end();
  }
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent — fires 3 SELECT queries in parallel
app.get("/concurrent", async (c) => {
  const pool = getPool();
  try {
    const [r1, r2, r3] = await Promise.all([
      pool.query("SELECT 1 AS n"),
      pool.query("SELECT 2 AS n"),
      pool.query("SELECT 3 AS n"),
    ]);
    return c.json({
      ok: true,
      results: [r1.rows[0].n, r2.rows[0].n, r3.rows[0].n],
      sum: Number(r1.rows[0].n) + Number(r2.rows[0].n) + Number(r3.rows[0].n),
    });
  } finally {
    await pool.end();
  }
});

Deno.serve(app.fetch);
