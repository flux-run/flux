import { drizzle } from "npm:drizzle-orm@0.45.1/node-postgres";
import { sql } from "npm:drizzle-orm@0.45.1";
import pg from "./flux-pg.js";

type Input = {
  connectionString: string;
  name?: string;
};

export default async function handler({ input }: { input: Input }) {
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