import { Hono } from "npm:hono";
import { ZodError } from "npm:zod";

import {
  createDispatchSchema,
  type CreateDispatchInput,
} from "./schema_flux.ts";
import type { DispatchRepository } from "./db_flux.ts";

function validationError(error: ZodError) {
  return {
    error: "Validation failed",
    issues: error.issues.map((issue) => ({
      path: issue.path.join("."),
      message: issue.message,
    })),
  };
}

async function parseDispatchInput(request: Request): Promise<CreateDispatchInput> {
  const payload = await request.json();
  return createDispatchSchema.parse(payload);
}

function remoteBaseUrl(): string {
  return Deno.env.get("REMOTE_BASE_URL") ?? "http://127.0.0.1:9010";
}

export function createApp(repository: DispatchRepository) {
  const app = new Hono();

  app.get("/", (c) =>
    c.json({
      name: "db_then_remote",
      endpoints: ["GET /dispatches", "POST /dispatches"],
      remoteBaseUrl: remoteBaseUrl(),
    }),
  );

  app.get("/dispatches", async (c) => {
    const rows = await repository.list();
    return c.json(rows);
  });

  app.post("/dispatches", async (c) => {
    try {
      const input = await parseDispatchInput(c.req.raw);
      const pending = await repository.createPending(input);

      const remoteResponse = await fetch(`${remoteBaseUrl()}/ingest`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          dispatchId: pending.id,
          orderId: pending.orderId,
          message: pending.message,
        }),
      });

      if (!remoteResponse.ok) {
        const remoteBody = await remoteResponse.text();
        throw new Error(
          `remote system returned ${remoteResponse.status}: ${remoteBody}`,
        );
      }

      const delivered = await repository.markDelivered(
        pending.id,
        remoteResponse.status,
      );

      if (!delivered) {
        throw new Error(`dispatch ${pending.id} vanished before completion`);
      }

      return c.json(
        {
          dispatch: delivered,
          remote: {
            status: remoteResponse.status,
          },
        },
        201,
      );
    } catch (error) {
      if (error instanceof ZodError) {
        return c.json(validationError(error), 422);
      }

      if (error instanceof SyntaxError) {
        return c.json({ error: "Request body must be valid JSON" }, 400);
      }

      throw error;
    }
  });

  return app;
}