import { Hono } from "hono";
import { z } from "zod";
import { zValidator } from "@hono/zod-validator";
import pg from "flux:pg";

const app = new Hono();

/**
 * Database setup (Flux-native Postgres)
 */
const pool = new pg.Pool({
  connectionString: Deno.env.get("DATABASE_URL"),
});

/**
 * Schema: create orders table (run once manually in DB)
 *
 * CREATE TABLE orders (
 *   id SERIAL PRIMARY KEY,
 *   email TEXT NOT NULL,
 *   amount INTEGER NOT NULL,
 *   status TEXT NOT NULL,
 *   created_at TIMESTAMP DEFAULT NOW()
 * );
 */

/**
 * Health check
 */
app.get("/", (c) => {
  return c.json({
    status: "ok",
    service: "flux-demo",
    timestamp: new Date().toISOString(),
  });
});

/**
 * Create Order (real-world pattern)
 * - validates input
 * - writes to DB
 * - calls external API (mock payment)
 */
const createOrderSchema = z.object({
  email: z.string().email(),
  amount: z.number().min(1),
});

app.post("/orders", zValidator("json", createOrderSchema), async (c) => {
  const { email, amount } = c.req.valid("json");

  // 1. Insert order (pending)
  const insertResult = await pool.query(
    `INSERT INTO orders (email, amount, status)
     VALUES ($1, $2, 'pending')
     RETURNING id`,
    [email, amount],
  );

  const orderId = insertResult.rows[0].id;

  // 2. Call external API (simulated payment)
  const paymentRes = await fetch("https://httpbin.org/post", {
    method: "POST",
    body: JSON.stringify({ orderId, amount }),
    headers: { "content-type": "application/json" },
  });

  const paymentData = await paymentRes.json();

  // 3. Update order status
  await pool.query(`UPDATE orders SET status = 'completed' WHERE id = $1`, [
    orderId,
  ]);

  return c.json({
    orderId,
    status: "completed",
    paymentRef: paymentData.url,
  });
});

/**
 * Get all orders
 */
app.get("/orders", async (c) => {
  const result = await pool.query(`SELECT * FROM orders ORDER BY id DESC`);
  return c.json(result.rows);
});

/**
 * Failure route (important for Flux demo)
 */
app.get("/fail", async () => {
  // simulate partial work
  await pool.query(
    `INSERT INTO orders (email, amount, status)
     VALUES ('fail@test.com', 100, 'pending')`,
  );

  throw new Error("Simulated failure after DB write");
});

Deno.serve(app.fetch);
