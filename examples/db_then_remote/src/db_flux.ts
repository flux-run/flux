import pg from "../../flux-pg.js";

import {
  dispatchRecordSchema,
  type CreateDispatchInput,
  type DispatchRecord,
} from "./schema_flux.ts";

type FluxPool = {
  query: (
    query:
      | string
      | { text: string; values?: unknown[]; rowMode?: "array" },
    params?: unknown[],
  ) => Promise<{ rows: Record<string, unknown>[]; rowCount?: number }>;
  end: () => Promise<void>;
};

export type DispatchRepository = {
  list: () => Promise<DispatchRecord[]>;
  createPending: (input: CreateDispatchInput) => Promise<DispatchRecord>;
  markDelivered: (id: number, remoteStatus: number) => Promise<DispatchRecord | null>;
  pool: FluxPool;
};

export function createDispatchRepository(): DispatchRepository {
  const databaseUrl = Deno.env.get("DATABASE_URL");

  if (!databaseUrl) {
    throw new Error("DATABASE_URL is required to run the db_then_remote example.");
  }

  const pool = new pg.Pool({
    connectionString: databaseUrl,
  }) as FluxPool;

  return {
    pool,

    async list() {
      const result = await pool.query(
        `
          SELECT id, order_id, message, status, remote_status, created_at, delivered_at
          FROM outbound_dispatches
          ORDER BY id DESC
        `,
      );

      return result.rows.map(mapDispatchRow);
    },

    async createPending(input) {
      const result = await pool.query(
        `
          INSERT INTO outbound_dispatches (order_id, message, status)
          VALUES ($1, $2, 'pending')
          RETURNING id, order_id, message, status, remote_status, created_at, delivered_at
        `,
        [input.orderId, input.message],
      );

      return mapDispatchRow(result.rows[0]);
    },

    async markDelivered(id, remoteStatus) {
      const result = await pool.query(
        `
          UPDATE outbound_dispatches
          SET status = 'delivered', remote_status = $2, delivered_at = now()
          WHERE id = $1
          RETURNING id, order_id, message, status, remote_status, created_at, delivered_at
        `,
        [id, remoteStatus],
      );

      const [row] = result.rows;
      return row ? mapDispatchRow(row) : null;
    },
  };
}

function mapDispatchRow(row: Record<string, unknown>): DispatchRecord {
  return dispatchRecordSchema.parse({
    id: row.id,
    orderId: row.order_id,
    message: row.message,
    status: row.status,
    remoteStatus: row.remote_status ?? null,
    createdAt: stringifyTimestamp(row.created_at),
    deliveredAt: row.delivered_at ? stringifyTimestamp(row.delivered_at) : null,
  });
}

function stringifyTimestamp(value: unknown): string {
  if (value instanceof Date) {
    return value.toISOString();
  }

  return String(value);
}