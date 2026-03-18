import pg from "../flux-pg.js";

type FluxPool = {
  query: (
    query: string | { text: string; values?: unknown[]; rowMode?: "array" },
    params?: unknown[],
  ) => Promise<{ rows: Record<string, unknown>[]; rowCount?: number }>;
  end: () => Promise<void>;
};

type CreateOrderInput = {
  sku: string;
  quantity: number;
};

type OrderRecord = {
  id: number;
  idempotencyKey: string;
  sku: string;
  quantity: number;
  createdAt: string;
};

type StoredResponse = {
  status: number;
  body: {
    order: OrderRecord;
  };
};

const databaseUrl = Deno.env.get("DATABASE_URL");
if (!databaseUrl) {
  throw new Error("DATABASE_URL is required to run the idempotency example.");
}

const redisUrl = Deno.env.get("REDIS_URL");
if (!redisUrl) {
  throw new Error("REDIS_URL is required to run the idempotency example.");
}

const pool = new pg.Pool({
  connectionString: databaseUrl,
}) as FluxPool;

const redis = Flux.redis.createClient({ url: redisUrl });
await redis.connect();

Deno.serve(async (request) => {
  const url = new URL(request.url);

  if (request.method === "GET" && url.pathname === "/orders") {
    return listOrders();
  }

  if (request.method === "POST" && url.pathname === "/orders") {
    return createOrderWithIdempotency(request);
  }

  return jsonResponse({ error: "not found" }, 404);
});

async function listOrders(): Promise<Response> {
  const result = await pool.query(
    `
      SELECT id, idempotency_key, sku, quantity, created_at
      FROM idempotent_orders
      ORDER BY id ASC
    `,
  );

  return jsonResponse({
    orders: result.rows.map(mapOrderRow),
  });
}

async function createOrderWithIdempotency(request: Request): Promise<Response> {
  const idempotencyKey = request.headers.get("idempotency-key")?.trim() ?? "";
  if (!idempotencyKey) {
    return jsonResponse({ error: "missing idempotency-key header" }, 400);
  }

  const redisKey = `idempotency:${idempotencyKey}`;
  const cached = await redis.get(redisKey);
  if (cached) {
    return responseFromStored(parseStoredResponse(cached), "replayed");
  }

  const payload = await parseCreateOrderInput(request);
  if (payload instanceof Response) {
    return payload;
  }

  const order = await insertOrLoadOrder(idempotencyKey, payload);
  const storedResponse: StoredResponse = {
    status: 201,
    body: { order },
  };

  await redis.set(redisKey, JSON.stringify(storedResponse));
  await redis.expire(redisKey, 60 * 60);

  return responseFromStored(storedResponse, "created");
}

async function parseCreateOrderInput(request: Request): Promise<CreateOrderInput | Response> {
  let body: unknown;
  try {
    body = await request.json();
  } catch {
    return jsonResponse({ error: "invalid JSON body" }, 400);
  }

  const input = body as Partial<CreateOrderInput>;
  if (typeof input.sku !== "string" || input.sku.trim().length === 0) {
    return jsonResponse({ error: "sku must be a non-empty string" }, 422);
  }
  if (!Number.isInteger(input.quantity) || (input.quantity ?? 0) <= 0) {
    return jsonResponse({ error: "quantity must be a positive integer" }, 422);
  }

  return {
    sku: input.sku.trim(),
    quantity: input.quantity,
  };
}

async function insertOrLoadOrder(
  idempotencyKey: string,
  input: CreateOrderInput,
): Promise<OrderRecord> {
  try {
    const result = await pool.query(
      `
        INSERT INTO idempotent_orders (idempotency_key, sku, quantity)
        VALUES ($1, $2, $3)
        RETURNING id, idempotency_key, sku, quantity, created_at
      `,
      [idempotencyKey, input.sku, input.quantity],
    );
    return mapOrderRow(result.rows[0]);
  } catch (error) {
    const code = (error as { code?: string } | null)?.code;
    if (code !== "23505") {
      throw error;
    }

    const existing = await pool.query(
      `
        SELECT id, idempotency_key, sku, quantity, created_at
        FROM idempotent_orders
        WHERE idempotency_key = $1
        LIMIT 1
      `,
      [idempotencyKey],
    );

    const row = existing.rows[0];
    if (!row) {
      throw error;
    }

    return mapOrderRow(row);
  }
}

function parseStoredResponse(serialized: string): StoredResponse {
  const parsed = JSON.parse(serialized) as Partial<StoredResponse>;
  if (typeof parsed.status !== "number" || !parsed.body || typeof parsed.body !== "object") {
    throw new Error("invalid stored idempotency response");
  }
  return parsed as StoredResponse;
}

function responseFromStored(stored: StoredResponse, status: "created" | "replayed"): Response {
  return jsonResponse(stored.body, stored.status, {
    "x-idempotency-status": status,
  });
}

function jsonResponse(
  body: unknown,
  status = 200,
  headers: HeadersInit = {},
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      ...headers,
    },
  });
}

function mapOrderRow(row: Record<string, unknown>): OrderRecord {
  return {
    id: Number(row.id),
    idempotencyKey: String(row.idempotency_key),
    sku: String(row.sku),
    quantity: Number(row.quantity),
    createdAt: String(row.created_at),
  };
}