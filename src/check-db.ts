// @ts-nocheck
import pg from "flux:pg";

const DB_URL = Deno.env.get("DATABASE_URL");
const pool = new pg.Pool({ connectionString: DB_URL });

export default async function handler() {
  const result = await pool.query("SELECT * FROM flux_demo_orders ORDER BY id DESC LIMIT 5");
  console.log("Latest Orders:", result.rows);
  return result.rows;
}
