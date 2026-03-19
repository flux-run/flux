// @ts-nocheck
// Compat test: Drizzle ORM over pg via Flux.postgres — exhaustive coverage
// Tests: setup, insert, select, update, delete, where clauses, ordering, pagination,
//        transactions, constraint errors, concurrent queries, joins, JSON columns
import { Hono } from "npm:hono";
import pg from "flux:pg";
import { drizzle } from "npm:drizzle-orm/node-postgres";
import {
  pgTable, serial, text, integer, boolean, jsonb, timestamp, primaryKey
} from "npm:drizzle-orm/pg-core";
import { eq, gt, lt, gte, lte, and, or, like, desc, asc, sql, inArray } from "npm:drizzle-orm";

// ── Schema ─────────────────────────────────────────────────────────────────

const users = pgTable("flux_drizzle_users", {
  id: serial("id").primaryKey(),
  name: text("name").notNull(),
  email: text("email").notNull().unique(),
  score: integer("score").default(0),
  active: boolean("active").default(true),
  meta: jsonb("meta"),
  createdAt: timestamp("created_at").defaultNow(),
});

const posts = pgTable("flux_drizzle_posts", {
  id: serial("id").primaryKey(),
  userId: integer("user_id").notNull().references(() => users.id),
  title: text("title").notNull(),
  body: text("body"),
  createdAt: timestamp("created_at").defaultNow(),
});

const app = new Hono();

function getDb() {
  const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
  return { db: drizzle(pool), pool };
}

// ── Setup / teardown ──────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "drizzle", ok: true }));

app.post("/setup", async (c) => {
  const { db, pool } = getDb();
  await db.execute(`
    CREATE TABLE IF NOT EXISTS flux_drizzle_users (
      id        SERIAL PRIMARY KEY,
      name      TEXT NOT NULL,
      email     TEXT NOT NULL UNIQUE,
      score     INT DEFAULT 0,
      active    BOOLEAN DEFAULT TRUE,
      meta      JSONB,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  await db.execute(`
    CREATE TABLE IF NOT EXISTS flux_drizzle_posts (
      id        SERIAL PRIMARY KEY,
      user_id   INT NOT NULL REFERENCES flux_drizzle_users(id) ON DELETE CASCADE,
      title     TEXT NOT NULL,
      body      TEXT,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  await pool.end();
  return c.json({ ok: true });
});

app.delete("/cleanup", async (c) => {
  const { db, pool } = getDb();
  await db.execute("DROP TABLE IF EXISTS flux_drizzle_posts CASCADE");
  await db.execute("DROP TABLE IF EXISTS flux_drizzle_users CASCADE");
  await pool.end();
  return c.json({ ok: true });
});

// ── CRUD: Users ───────────────────────────────────────────────────────────

// POST /users — insert a user
app.post("/users", async (c) => {
  const body = await c.req.json();
  const { db, pool } = getDb();
  try {
    const result = await db
      .insert(users)
      .values({ name: body.name, email: body.email, score: body.score ?? 0, meta: body.meta })
      .returning();
    return c.json({ ok: true, user: result[0] });
  } finally { await pool.end(); }
});

// GET /users — list all users
app.get("/users", async (c) => {
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).orderBy(asc(users.id));
    return c.json({ ok: true, count: rows.length, users: rows });
  } finally { await pool.end(); }
});

// GET /users/:id — get by id
app.get("/users/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).where(eq(users.id, id));
    if (!rows.length) return c.json({ ok: false, error: "not found" }, 404);
    return c.json({ ok: true, user: rows[0] });
  } finally { await pool.end(); }
});

// PUT /users/:id — update user by id
app.put("/users/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const body = await c.req.json();
  const { db, pool } = getDb();
  try {
    const result = await db
      .update(users)
      .set({ name: body.name, score: body.score, active: body.active })
      .where(eq(users.id, id))
      .returning();
    return c.json({ ok: true, user: result[0] });
  } finally { await pool.end(); }
});

// DELETE /users/:id — delete user
app.delete("/users/:id", async (c) => {
  const id = Number(c.req.param("id"));
  const { db, pool } = getDb();
  try {
    const result = await db.delete(users).where(eq(users.id, id)).returning();
    return c.json({ ok: true, deleted: result.length > 0 });
  } finally { await pool.end(); }
});

// ── CRUD: Posts ───────────────────────────────────────────────────────────

app.post("/posts", async (c) => {
  const body = await c.req.json();
  const { db, pool } = getDb();
  try {
    const result = await db
      .insert(posts)
      .values({ userId: body.userId, title: body.title, body: body.body })
      .returning();
    return c.json({ ok: true, post: result[0] });
  } finally { await pool.end(); }
});

app.get("/posts", async (c) => {
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(posts).orderBy(desc(posts.createdAt));
    return c.json({ ok: true, count: rows.length, posts: rows });
  } finally { await pool.end(); }
});

// ── Filtering & ordering ──────────────────────────────────────────────────

// GET /users/active — only active users
app.get("/filter/active", async (c) => {
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).where(eq(users.active, true));
    return c.json({ ok: true, count: rows.length, all_active: rows.every((r) => r.active) });
  } finally { await pool.end(); }
});

// GET /filter/score-gt — users with score > threshold
app.get("/filter/score-gt", async (c) => {
  const threshold = Number(c.req.query("min") ?? 50);
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).where(gt(users.score, threshold));
    return c.json({ ok: true, count: rows.length, threshold });
  } finally { await pool.end(); }
});

// GET /filter/compound — active AND score >= min
app.get("/filter/compound", async (c) => {
  const min = Number(c.req.query("min") ?? 0);
  const { db, pool } = getDb();
  try {
    const rows = await db
      .select()
      .from(users)
      .where(and(eq(users.active, true), gte(users.score, min)));
    return c.json({ ok: true, count: rows.length });
  } finally { await pool.end(); }
});

// GET /filter/name-like — LIKE search on name
app.get("/filter/name-like", async (c) => {
  const pattern = c.req.query("q") ?? "%";
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).where(like(users.name, `%${pattern}%`));
    return c.json({ ok: true, count: rows.length, pattern });
  } finally { await pool.end(); }
});

// GET /filter/in — users with id in list
app.get("/filter/in", async (c) => {
  const ids = (c.req.query("ids") ?? "").split(",").map(Number).filter(Boolean);
  if (!ids.length) return c.json({ ok: false, error: "ids param required" }, 400);
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).where(inArray(users.id, ids));
    return c.json({ ok: true, count: rows.length, ids_found: rows.map((r) => r.id) });
  } finally { await pool.end(); }
});

// GET /order/desc — descending by score
app.get("/order/desc", async (c) => {
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).orderBy(desc(users.score));
    const scores = rows.map((r) => r.score ?? 0);
    const sorted = [...scores].sort((a, b) => b - a);
    return c.json({ ok: true, scores, is_sorted: JSON.stringify(scores) === JSON.stringify(sorted) });
  } finally { await pool.end(); }
});

// GET /paginate — LIMIT + OFFSET
app.get("/paginate", async (c) => {
  const page = Number(c.req.query("page") ?? 1);
  const limit = Number(c.req.query("limit") ?? 5);
  const offset = (page - 1) * limit;
  const { db, pool } = getDb();
  try {
    const rows = await db.select().from(users).limit(limit).offset(offset).orderBy(asc(users.id));
    return c.json({ ok: true, page, limit, count: rows.length, rows });
  } finally { await pool.end(); }
});

// ── Aggregates ────────────────────────────────────────────────────────────

// GET /aggregate — COUNT, AVG, MAX on score
app.get("/aggregate", async (c) => {
  const { db, pool } = getDb();
  try {
    const r = await db
      .select({
        count: sql<number>`COUNT(*)`,
        avg: sql<number>`AVG(score)`,
        max: sql<number>`MAX(score)`,
        min: sql<number>`MIN(score)`,
      })
      .from(users);
    return c.json({ ok: true, stats: r[0] });
  } finally { await pool.end(); }
});

// ── JSONB ─────────────────────────────────────────────────────────────────

// GET /jsonb — insert with JSONB meta + query jsonb operator
app.get("/jsonb", async (c) => {
  const { db, pool } = getDb();
  try {
    const meta = { role: "admin", preferences: { theme: "dark" }, flags: [1, 2, 3] };
    const ins = await db
      .insert(users)
      .values({ name: "jsonb-user", email: `jsonb-${Date.now()}@test.com`, meta })
      .returning();
    const row = ins[0];
    // query back using jsonb operator
    const found = await db
      .select()
      .from(users)
      .where(sql`meta->>'role' = 'admin'`);
    await db.delete(users).where(eq(users.id, row.id));
    return c.json({
      ok: true,
      meta_stored: row.meta,
      found_by_jsonb_op: found.length > 0,
    });
  } finally { await pool.end(); }
});

// ── Transactions ──────────────────────────────────────────────────────────

// POST /transaction — insert user + post in one transaction
app.post("/transaction", async (c) => {
  const body = await c.req.json();
  const { pool } = getDb();
  const client = await pool.connect();
  const db2 = drizzle(client);
  try {
    await client.query("BEGIN");
    const userResult = await db2
      .insert(users)
      .values({ name: body.name, email: `txn-${Date.now()}@test.com` })
      .returning();
    const user = userResult[0];
    const postResult = await db2
      .insert(posts)
      .values({ userId: user.id, title: body.postTitle ?? "TXN Post" })
      .returning();
    await client.query("COMMIT");
    return c.json({ ok: true, user, post: postResult[0] });
  } catch (e) {
    await client.query("ROLLBACK");
    return c.json({ ok: false, error: String(e) }, 500);
  } finally {
    client.release();
    await pool.end();
  }
});

// GET /transaction-rollback — error in tx rolls back both
app.get("/transaction-rollback", async (c) => {
  const { pool } = getDb();
  const client = await pool.connect();
  const db2 = drizzle(client);
  const email = `rollback-${Date.now()}@test.com`;
  try {
    await client.query("BEGIN");
    await db2.insert(users).values({ name: "RollbackUser", email });
    // duplicate email will violate UNIQUE → triggers catch → ROLLBACK
    await db2.insert(users).values({ name: "DupUser", email });
    await client.query("COMMIT");
    return c.json({ ok: false, error: "expected constraint error" }, 500);
  } catch (_e) {
    await client.query("ROLLBACK");
    const check = await pool.query("SELECT COUNT(*) AS cnt FROM flux_drizzle_users WHERE email = $1", [email]);
    return c.json({ ok: true, rolled_back: Number(check.rows[0]?.cnt) === 0 });
  } finally {
    client.release();
    await pool.end();
  }
});

// ── Constraint errors ──────────────────────────────────────────────────────

// POST /unique-email — duplicate email returns pg error 23505
app.post("/unique-email", async (c) => {
  const { db, pool } = getDb();
  try {
    const email = `unique-${Date.now()}@test.com`;
    await db.insert(users).values({ name: "A", email });
    try {
      await db.insert(users).values({ name: "B", email });
      return c.json({ ok: false }, 500);
    } catch (e: any) {
      return c.json({ ok: true, caught: true, code: e?.code, is_unique_violation: e?.code === "23505" });
    }
  } finally {
    await pool.query("DELETE FROM flux_drizzle_users WHERE email LIKE 'unique-%'").catch(() => {});
    await pool.end();
  }
});

// ── Join ──────────────────────────────────────────────────────────────────

// GET /join — users joined with their posts
app.get("/join", async (c) => {
  const { db, pool } = getDb();
  try {
    const rows = await db
      .select({
        userName: users.name,
        postTitle: posts.title,
      })
      .from(users)
      .leftJoin(posts, eq(users.id, posts.userId))
      .orderBy(asc(users.id));
    return c.json({ ok: true, count: rows.length, rows });
  } finally { await pool.end(); }
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent — 5 simultaneous select queries
app.get("/concurrent", async (c) => {
  const { db, pool } = getDb();
  try {
    const queries = Array.from({ length: 5 }, () => db.select().from(users).limit(3));
    const results = await Promise.all(queries);
    return c.json({ ok: true, results_count: results.length });
  } finally { await pool.end(); }
});

Deno.serve(app.fetch);
