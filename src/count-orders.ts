// @ts-nocheck
import pg from "flux:pg";

const DB_URL = Deno.env.get("DATABASE_URL");
const pool = new pg.Pool({ connectionString: DB_URL });

export default async function handler() {
  const result = await pool.query("SELECT COUNT(*) as count FROM flux_demo_orders");
  console.log("COUNT:", result.rows[0].count);
  return { count: result.rows[0].count };
}
