import { eq, sql } from "drizzle-orm";
import { integer, pgTable, text } from "drizzle-orm/pg-core";
import { drizzle } from "drizzle-orm/node-postgres";
import { Pool } from "pg";

type Input = {
  connectionString: string;
};

const jobs = pgTable("flux_drizzle_jobs", {
  id: integer("id").primaryKey().generatedAlwaysAsIdentity(),
  name: text("name").notNull(),
  status: text("status").notNull(),
});

export default async function handler({ input }: { input: Input }) {
  const pool = new Pool({ connectionString: input.connectionString });
  const db = drizzle(pool);

  try {
    await db.execute(sql`drop table if exists flux_drizzle_jobs`);
    await db.execute(sql`
      create table flux_drizzle_jobs (
        id integer generated always as identity primary key,
        name text not null,
        status text not null
      )
    `);

    const txResult = await db.transaction(async (tx) => {
      const inserted = await tx
        .insert(jobs)
        .values({ name: "replay-check", status: "queued" })
        .returning();

      const selected = await tx
        .select()
        .from(jobs)
        .where(eq(jobs.id, inserted[0].id));

      const updated = await tx
        .update(jobs)
        .set({ status: "running" })
        .where(eq(jobs.id, inserted[0].id))
        .returning();

      return {
        inserted: inserted[0],
        selected: selected[0],
        updated: updated[0],
      };
    });

    const finalRows = await db.select().from(jobs).orderBy(jobs.id);

    return {
      txResult,
      finalRows,
    };
  } finally {
    try {
      await db.execute(sql`drop table if exists flux_drizzle_jobs`);
    } catch {
      // Best-effort cleanup so reruns stay simple.
    }
    await pool.end();
  }
}