import pg from "../flux-pg.js";

type FluxPool = {
  query: (
    query: string | { text: string; values?: unknown[]; rowMode?: "array" },
    params?: unknown[],
  ) => Promise<{ rows: Record<string, unknown>[]; rowCount?: number }>;
  end: () => Promise<void>;
};

type WebhookInput = {
  provider: string;
  type: string;
};

type EventRecord = {
  id: number;
  eventId: string;
  provider: string;
  type: string;
  receivedAt: string;
};

const databaseUrl = Deno.env.get("DATABASE_URL");
if (!databaseUrl) {
  throw new Error("DATABASE_URL is required to run the webhook dedup example.");
}

const redisUrl = Deno.env.get("REDIS_URL");
if (!redisUrl) {
  throw new Error("REDIS_URL is required to run the webhook dedup example.");
}

const pool = new pg.Pool({
  connectionString: databaseUrl,
}) as FluxPool;

const redis = Flux.redis.createClient({ url: redisUrl });
await redis.connect();

Deno.serve(async (request) => {
  const url = new URL(request.url);

  if (request.method === "GET" && url.pathname === "/events") {
    return listEvents();
  }

  if (request.method === "POST" && url.pathname === "/webhook") {
    return handleWebhook(request);
  }

  return jsonResponse({ error: "not found" }, 404);
});

async function listEvents(): Promise<Response> {
  const result = await pool.query(
    `
      SELECT id, event_id, provider, event_type, received_at
      FROM webhook_events
      ORDER BY id ASC
    `,
  );

  return jsonResponse({ events: result.rows.map(mapEventRow) });
}

async function handleWebhook(request: Request): Promise<Response> {
  const eventId = request.headers.get("x-event-id")?.trim() ?? "";
  if (!eventId) {
    return jsonResponse({ error: "missing x-event-id header" }, 400);
  }

  const seenKey = `event:${eventId}`;
  const seen = await redis.get(seenKey);
  if (seen) {
    return jsonResponse(
      { status: "duplicate", eventId },
      200,
      { "x-webhook-status": "duplicate" },
    );
  }

  const payload = await parseWebhookInput(request);
  if (payload instanceof Response) {
    return payload;
  }

  const event = await insertOrLoadEvent(eventId, payload);
  await redis.set(seenKey, "1");
  await redis.expire(seenKey, 60 * 60);

  return jsonResponse(
    { status: "processed", eventId, event },
    202,
    { "x-webhook-status": "processed" },
  );
}

async function parseWebhookInput(request: Request): Promise<WebhookInput | Response> {
  let body: unknown;
  try {
    body = await request.json();
  } catch {
    return jsonResponse({ error: "invalid JSON body" }, 400);
  }

  const input = body as Partial<WebhookInput>;
  if (typeof input.provider !== "string" || input.provider.trim().length === 0) {
    return jsonResponse({ error: "provider must be a non-empty string" }, 422);
  }
  if (typeof input.type !== "string" || input.type.trim().length === 0) {
    return jsonResponse({ error: "type must be a non-empty string" }, 422);
  }

  return {
    provider: input.provider.trim(),
    type: input.type.trim(),
  };
}

async function insertOrLoadEvent(eventId: string, input: WebhookInput): Promise<EventRecord> {
  try {
    const result = await pool.query(
      `
        INSERT INTO webhook_events (event_id, provider, event_type)
        VALUES ($1, $2, $3)
        RETURNING id, event_id, provider, event_type, received_at
      `,
      [eventId, input.provider, input.type],
    );
    return mapEventRow(result.rows[0]);
  } catch (error) {
    const code = (error as { code?: string } | null)?.code;
    if (code !== "23505") {
      throw error;
    }

    const existing = await pool.query(
      `
        SELECT id, event_id, provider, event_type, received_at
        FROM webhook_events
        WHERE event_id = $1
        LIMIT 1
      `,
      [eventId],
    );

    const row = existing.rows[0];
    if (!row) {
      throw error;
    }

    return mapEventRow(row);
  }
}

function jsonResponse(body: unknown, status = 200, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      ...headers,
    },
  });
}

function mapEventRow(row: Record<string, unknown>): EventRecord {
  return {
    id: Number(row.id),
    eventId: String(row.event_id),
    provider: String(row.provider),
    type: String(row.event_type),
    receivedAt: String(row.received_at),
  };
}