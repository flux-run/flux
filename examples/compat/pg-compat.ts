// @ts-nocheck
// Compat test: pg (node-postgres) — exhaustive coverage
// Tests: SELECT, INSERT, UPDATE, DELETE, transactions, rollback, constraints,
//        parameterized queries, JSONB, arrays, NULL, concurrent queries, pooling
import { Hono } from "npm:hono";
import pg from "flux:pg";

const app = new Hono();

function getPool() {
  return new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
}

// Table used across tests (idempotent CREATE)
const DDL = `
  CREATE TABLE IF NOT EXISTS flux_pg_test (
    id        SERIAL PRIMARY KEY,
    label     TEXT NOT NULL,
    score     INT DEFAULT 0,
    meta      JSONB,
    tags      TEXT[],
    active    BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ DEFAULT NOW()
  )
`;

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "pg", ok: true }));

// ── Setup/teardown ─────────────────────────────────────────────────────────

app.post("/setup", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  await pool.end();
  return c.json({ ok: true });
});

app.delete("/cleanup", async (c) => {
  const pool = getPool();
  await pool.query("DROP TABLE IF EXISTS flux_pg_test");
  await pool.end();
  return c.json({ ok: true });
});

// ── Basic queries ─────────────────────────────────────────────────────────

// GET /select-1 — simplest possible query
app.get("/select-1", async (c) => {
  const pool = getPool();
  const r = await pool.query("SELECT 1 AS value");
  await pool.end();
  return c.json({ ok: true, value: Number(r.rows[0]?.value) });
});

// GET /now — server timestamp (determinism patched)
app.get("/now", async (c) => {
  const pool = getPool();
  const r = await pool.query("SELECT NOW() AS ts");
  await pool.end();
  return c.json({ ok: true, ts_present: !!r.rows[0]?.ts });
});

// GET /version — postgres version string
app.get("/version", async (c) => {
  const pool = getPool();
  const r = await pool.query("SELECT version() AS v");
  await pool.end();
  return c.json({ ok: true, version: r.rows[0]?.v?.slice(0, 10) });
});

// ── CRUD ──────────────────────────────────────────────────────────────────

// POST /insert — parameterized INSERT RETURNING
app.post("/insert", async (c) => {
  const { label, score, meta, tags } = await c.req.json();
  const pool = getPool();
  await pool.query(DDL);
  const r = await pool.query(
    "INSERT INTO flux_pg_test (label, score, meta, tags) VALUES ($1, $2, $3, $4) RETURNING *",
    [label, score ?? 0, meta ? JSON.stringify(meta) : null, tags ?? null],
  );
  await pool.end();
  return c.json({ ok: true, row: r.rows[0] });
});

// GET /select-all — SELECT all rows
app.get("/select-all", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  const r = await pool.query("SELECT * FROM flux_pg_test ORDER BY id");
  await pool.end();
  return c.json({ ok: true, count: r.rows.length, rows: r.rows });
});

// GET /select-where — SELECT with parameterized WHERE
app.get("/select-where", async (c) => {
  const label = c.req.query("label") ?? "test";
  const pool = getPool();
  await pool.query(DDL);
  const r = await pool.query("SELECT * FROM flux_pg_test WHERE label = $1", [label]);
  await pool.end();
  return c.json({ ok: true, count: r.rows.length, rows: r.rows });
});

// PUT /update — UPDATE by id
app.put("/update", async (c) => {
  const { id, label, score } = await c.req.json();
  const pool = getPool();
  const r = await pool.query(
    "UPDATE flux_pg_test SET label = $1, score = $2 WHERE id = $3 RETURNING *",
    [label, score, id],
  );
  await pool.end();
  return c.json({ ok: true, updated: r.rowCount, row: r.rows[0] });
});

// DELETE /delete — DELETE by id
app.delete("/delete-row", async (c) => {
  const { id } = await c.req.json();
  const pool = getPool();
  const r = await pool.query("DELETE FROM flux_pg_test WHERE id = $1 RETURNING id", [id]);
  await pool.end();
  return c.json({ ok: true, deleted: r.rowCount, id: r.rows[0]?.id });
});

// ── Data types ────────────────────────────────────────────────────────────

// GET /jsonb — JSONB column insert + retrieval
app.get("/jsonb", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  const meta = { key: "flux", nested: { count: 42 }, arr: [1, 2, 3] };
  const ins = await pool.query(
    "INSERT INTO flux_pg_test (label, meta) VALUES ($1, $2) RETURNING meta",
    ["jsonb-test", JSON.stringify(meta)],
  );
  const row = ins.rows[0];
  await pool.query("DELETE FROM flux_pg_test WHERE label = 'jsonb-test'");
  await pool.end();
  return c.json({ ok: true, meta: row?.meta, nested_count: row?.meta?.nested?.count });
});

// GET /arrays — TEXT[] arrays
app.get("/arrays", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  const tags = ["flux", "postgres", "deterministic"];
  const ins = await pool.query(
    "INSERT INTO flux_pg_test (label, tags) VALUES ($1, $2) RETURNING tags",
    ["array-test", tags],
  );
  const returned = ins.rows[0]?.tags;
  await pool.query("DELETE FROM flux_pg_test WHERE label = 'array-test'");
  await pool.end();
  return c.json({ ok: true, tags: returned, count: returned?.length });
});

// GET /null-values — NULL handling
app.get("/null-values", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  const ins = await pool.query(
    "INSERT INTO flux_pg_test (label, meta) VALUES ($1, NULL) RETURNING meta",
    ["null-test"],
  );
  const meta = ins.rows[0]?.meta;
  await pool.query("DELETE FROM flux_pg_test WHERE label = 'null-test'");
  await pool.end();
  return c.json({ ok: true, meta_is_null: meta === null });
});

// GET /boolean — BOOLEAN column
app.get("/boolean", async (c) => {
  const pool = getPool();
  await pool.query(DDL);
  const ins = await pool.query(
    "INSERT INTO flux_pg_test (label, active) VALUES ($1, $2) RETURNING active",
    ["bool-test", false],
  );
  const active = ins.rows[0]?.active;
  await pool.query("DELETE FROM flux_pg_test WHERE label = 'bool-test'");
  await pool.end();
  return c.json({ ok: true, active_is_false: active === false });
});

// ── Transactions ──────────────────────────────────────────────────────────

// GET /transaction-commit — successful transaction
app.get("/transaction-commit", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(DDL);
    const ins = await client.query(
      "INSERT INTO flux_pg_test (label) VALUES ($1) RETURNING id",
      ["txn-commit"],
    );
    await client.query("COMMIT");
    // verify it's visible outside the transaction
    const check = await pool.query("SELECT id FROM flux_pg_test WHERE id = $1", [ins.rows[0]?.id]);
    await pool.query("DELETE FROM flux_pg_test WHERE label = 'txn-commit'");
    return c.json({ ok: true, committed: check.rowCount === 1 });
  } catch (e) {
    await client.query("ROLLBACK");
    return c.json({ ok: false, error: String(e) }, 500);
  } finally {
    client.release();
    await pool.end();
  }
});

// GET /transaction-rollback — rollback discards data
app.get("/transaction-rollback", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query(DDL);
    await client.query("BEGIN");
    await client.query("INSERT INTO flux_pg_test (label) VALUES ($1)", ["txn-rollback"]);
    await client.query("ROLLBACK");
    const check = await client.query(
      "SELECT COUNT(*) AS cnt FROM flux_pg_test WHERE label = 'txn-rollback'",
    );
    return c.json({ ok: true, rows_after_rollback: Number(check.rows[0]?.cnt) });
  } finally {
    client.release();
    await pool.end();
  }
});

// GET /transaction-savepoint — SAVEPOINT + partial rollback
app.get("/transaction-savepoint", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query(DDL);
    await client.query("BEGIN");
    await client.query("INSERT INTO flux_pg_test (label) VALUES ($1)", ["save-before"]);
    await client.query("SAVEPOINT sp1");
    await client.query("INSERT INTO flux_pg_test (label) VALUES ($1)", ["save-after"]);
    await client.query("ROLLBACK TO SAVEPOINT sp1");
    await client.query("COMMIT");
    const r = await pool.query(
      "SELECT label FROM flux_pg_test WHERE label IN ('save-before','save-after')",
    );
    await pool.query("DELETE FROM flux_pg_test WHERE label IN ('save-before','save-after')");
    return c.json({
      ok: true,
      labels: r.rows.map((row) => row.label),
      before_committed: r.rows.some((row) => row.label === "save-before"),
      after_rolled_back: !r.rows.some((row) => row.label === "save-after"),
    });
  } finally {
    client.release();
    await pool.end();
  }
});

// ── Constraint handling ───────────────────────────────────────────────────

// POST /unique-violation — unique constraint returns pg error code 23505
app.post("/unique-violation", async (c) => {
  const pool = getPool();
  try {
    await pool.query(`
      CREATE TABLE IF NOT EXISTS flux_pg_unique_test (
        id SERIAL PRIMARY KEY, key TEXT UNIQUE NOT NULL
      )
    `);
    await pool.query("INSERT INTO flux_pg_unique_test (key) VALUES ($1)", ["dup-key"]);
    try {
      await pool.query("INSERT INTO flux_pg_unique_test (key) VALUES ($1)", ["dup-key"]);
      return c.json({ ok: false, error: "expected constraint error" }, 500);
    } catch (e: any) {
      return c.json({ ok: true, caught: true, pg_code: e?.code, is_23505: e?.code === "23505" });
    }
  } finally {
    await pool.query("DROP TABLE IF EXISTS flux_pg_unique_test").catch(() => {});
    await pool.end();
  }
});

// POST /not-null-violation — NOT NULL constraint (code 23502)
app.post("/not-null-violation", async (c) => {
  const pool = getPool();
  try {
    await pool.query(DDL);
    try {
      // label is NOT NULL
      await pool.query("INSERT INTO flux_pg_test (label) VALUES (NULL)");
      return c.json({ ok: false, error: "expected constraint error" }, 500);
    } catch (e: any) {
      return c.json({ ok: true, caught: true, pg_code: e?.code, is_23502: e?.code === "23502" });
    }
  } finally {
    await pool.end();
  }
});

// GET /syntax-error — invalid SQL returns error (code 42601)
app.get("/syntax-error", async (c) => {
  const pool = getPool();
  try {
    await pool.query("SELEKT 1");
    return c.json({ ok: false }, 500);
  } catch (e: any) {
    return c.json({ ok: true, caught: true, pg_code: e?.code });
  } finally {
    await pool.end();
  }
});

// ── Pool / client lifecycle ───────────────────────────────────────────────

// GET /pool-multiple — multiple queries through same pool
app.get("/pool-multiple", async (c) => {
  const pool = getPool();
  const results = await Promise.all([
    pool.query("SELECT 1 AS n"),
    pool.query("SELECT 2 AS n"),
    pool.query("SELECT 3 AS n"),
  ]);
  await pool.end();
  return c.json({
    ok: true,
    values: results.map((r) => Number(r.rows[0]?.n)),
    sum: results.reduce((acc, r) => acc + Number(r.rows[0]?.n), 0),
  });
});

// GET /client-connect — manual connect/release lifecycle
app.get("/client-connect", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    const r1 = await client.query("SELECT 10 AS a");
    const r2 = await client.query("SELECT 20 AS b");
    return c.json({
      ok: true,
      a: Number(r1.rows[0]?.a),
      b: Number(r2.rows[0]?.b),
      sum: Number(r1.rows[0]?.a) + Number(r2.rows[0]?.b),
    });
  } finally {
    client.release();
    await pool.end();
  }
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent — 3 simultaneous queries (runner expects sum=6 and results=[1,2,3])
app.get("/concurrent", async (c) => {
  const pool = getPool();
  try {
    const [r1, r2, r3] = await Promise.all([
      pool.query("SELECT 1 AS n"),
      pool.query("SELECT 2 AS n"),
      pool.query("SELECT 3 AS n"),
    ]);
    const results = [r1, r2, r3].map((r) => Number(r.rows[0]?.n));
    return c.json({
      ok: true,
      results,
      sum: results.reduce((a, b) => a + b, 0),
    });
  } finally {
    await pool.end();
  }
});

// ── Aliases expected by the integration test runner ───────────────────────

// GET /db-query — SELECT 1 (simplest query alias)
app.get("/db-query", async (c) => {
  const pool = getPool();
  const r = await pool.query("SELECT 1 AS value");
  await pool.end();
  return c.json({ ok: true, value: Number(r.rows[0]?.value) });
});

// POST /db-insert-select — insert a row then select it back in one request
app.post("/db-insert-select", async (c) => {
  const { label } = await c.req.json();
  const pool = getPool();
  await pool.query(DDL);
  const ins = await pool.query(
    "INSERT INTO flux_pg_test (label) VALUES ($1) RETURNING *",
    [label],
  );
  const inserted = ins.rows[0];
  const sel = await pool.query("SELECT * FROM flux_pg_test WHERE id = $1", [inserted?.id]);
  const selected = sel.rows[0];
  await pool.query("DELETE FROM flux_pg_test WHERE id = $1", [inserted?.id]);
  await pool.end();
  return c.json({ ok: true, inserted, selected });
});

// GET /db-transaction — transaction commit, returns ok + ts
app.get("/db-transaction", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(DDL);
    const r = await client.query("SELECT NOW() AS ts");
    await client.query("COMMIT");
    return c.json({ ok: true, ts: r.rows[0]?.ts });
  } catch (e) {
    await client.query("ROLLBACK");
    return c.json({ ok: false, error: String(e) }, 500);
  } finally {
    client.release();
    await pool.end();
  }
});

// GET /db-rollback — explicit rollback, returns rows_after_rollback
app.get("/db-rollback", async (c) => {
  const pool = getPool();
  const client = await pool.connect();
  try {
    await client.query(DDL);
    await client.query("BEGIN");
    await client.query("INSERT INTO flux_pg_test (label) VALUES ($1)", ["rollback-test"]);
    await client.query("ROLLBACK");
    const check = await client.query(
      "SELECT COUNT(*) AS cnt FROM flux_pg_test WHERE label = 'rollback-test'",
    );
    return c.json({ ok: true, rows_after_rollback: Number(check.rows[0]?.cnt) });
  } finally {
    client.release();
    await pool.end();
  }
});

// POST /db-constraint — unique constraint violation, returns caught=true + code=23505
app.post("/db-constraint", async (c) => {
  const pool = getPool();
  try {
    await pool.query(`
      CREATE TABLE IF NOT EXISTS flux_pg_constraint_test (
        id SERIAL PRIMARY KEY, key TEXT UNIQUE NOT NULL
      )
    `);
    await pool.query("INSERT INTO flux_pg_constraint_test (key) VALUES ($1)", ["dup-key-2"]);
    try {
      await pool.query("INSERT INTO flux_pg_constraint_test (key) VALUES ($1)", ["dup-key-2"]);
      return c.json({ ok: false }, 500);
    } catch (e: any) {
      return c.json({ ok: true, caught: true, code: e?.code });
    }
  } finally {
    await pool.query("DROP TABLE IF EXISTS flux_pg_constraint_test").catch(() => {});
    await pool.end();
  }
});

// GET /concurrent already exists above — but runner expects sum=6 and results=[1,2,3].
// Alias with 3 queries matching those expectations.
app.get("/pg-concurrent", async (c) => {
  const pool = getPool();
  try {
    const [r1, r2, r3] = await Promise.all([
      pool.query("SELECT 1 AS n"),
      pool.query("SELECT 2 AS n"),
      pool.query("SELECT 3 AS n"),
    ]);
    const results = [r1, r2, r3].map((r) => Number(r.rows[0]?.n));
    return c.json({
      ok: true,
      results,
      sum: results.reduce((a, b) => a + b, 0),
    });
  } finally {
    await pool.end();
  }
});

Deno.serve(app.fetch);
