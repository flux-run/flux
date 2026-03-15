# Example — Webhook Worker

Process incoming webhooks reliably: validate the signature, persist the event,
and dispatch processing — all in one Flux function.

---

## What you'll build

A webhook receiver that:

1. Verifies the webhook signature (HMAC-SHA256) before any processing
2. Stores the raw event in a `webhook_events` table
3. Dispatches type-specific handling (e.g. payment events, user events)
4. Returns a `200 OK` immediately after verification so the sender isn't blocked

---

## Schema

Create a `webhook_events` table:

| Column | Type |
|---|---|
| `id` | `uuid` (primary key, auto) |
| `source` | `text` — e.g. `"stripe"`, `"github"` |
| `event_type` | `text` — e.g. `"payment.succeeded"` |
| `payload` | `jsonb` — raw event body |
| `processed` | `boolean` — default `false` |
| `created_at` | `timestamptz` — auto |

---

## The function

Create `on_webhook/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";
import { createHmac, timingSafeEqual } from "node:crypto";

// Minimal signature check — works with Stripe, GitHub, and most providers
function verifySignature(
  payloadRaw: string,
  signature: string,
  secret: string,
): boolean {
  const expected = createHmac("sha256", secret)
    .update(payloadRaw)
    .digest("hex");
  // Prefix may vary by provider (Stripe uses "sha256=", GitHub uses "sha256=")
  const incoming = signature.replace(/^sha256=/, "");
  try {
    return timingSafeEqual(
      Buffer.from(expected, "hex"),
      Buffer.from(incoming, "hex"),
    );
  } catch {
    return false;
  }
}

export default defineFunction({
  name: "on_webhook",
  // Webhooks come in as a raw body + signature header in the payload wrapper
  input: z.object({
    source:    z.string(),                       // "stripe" | "github" | ...
    signature: z.string(),                       // X-Hub-Signature-256 / Stripe-Signature
    raw:       z.string(),                       // raw JSON string body
  }),
  output: z.object({ received: z.boolean() }),

  handler: async ({ input, ctx }) => {
    // 1. Verify signature before anything else
    const sigSecret = ctx.secrets.get(`${input.source.toUpperCase()}_WEBHOOK_SECRET`);
    if (!sigSecret) {
      ctx.log(`No webhook secret for source: ${input.source}`, "warn");
      throw new Error("Webhook source not configured");
    }

    if (!verifySignature(input.raw, input.signature, sigSecret)) {
      ctx.log("Webhook signature verification failed", "warn");
      throw new Error("Invalid signature");
    }

    const event = JSON.parse(input.raw);

    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    // 2. Persist the raw event
    const [record] = await flux.db.webhook_events
      .insert({
        source:     input.source,
        event_type: event.type ?? "unknown",
        payload:    event,
        processed:  false,
      })
      .returning(["id"])
      .execute();

    ctx.log(`Stored webhook event ${record.id} (${input.source}/${event.type})`);

    // 3. Dispatch type-specific handling inline (or enqueue async)
    try {
      await handleEvent(input.source, event, { flux, ctx });

      await flux.db.webhook_events
        .update({ processed: true })
        .where("id", "eq", record.id)
        .execute();
    } catch (err) {
      // Log the failure but still return 200 — the event is stored and
      // can be retried later via a cron job querying processed=false
      ctx.log(`Event handling failed: ${err}`, "error");
    }

    // 4. Always reply quickly
    return { received: true };
  },
});

// ─── Event dispatch ───────────────────────────────────────────────────────────

async function handleEvent(
  source: string,
  event: Record<string, unknown>,
  { flux, ctx }: { flux: ReturnType<typeof createClient>; ctx: FluxContext },
) {
  switch (`${source}/${event.type}`) {
    case "stripe/payment_intent.succeeded": {
      const amount = (event.data as any)?.object?.amount;
      ctx.log(`Payment succeeded — amount: ${amount}`);
      // update orders table, send confirmation email, etc.
      break;
    }
    case "github/push": {
      const ref = event.ref as string;
      ctx.log(`Push to ${ref}`);
      // trigger a build pipeline, notify Slack, etc.
      break;
    }
    default:
      ctx.log(`Unhandled event: ${source}/${event.type}`, "warn");
  }
}
```

---

## Step 3 — Set secrets

```bash
flux secrets set STRIPE_WEBHOOK_SECRET   whsec_...
flux secrets set GITHUB_WEBHOOK_SECRET   ghw_...
flux secrets set GATEWAY_URL             "https://YOUR_GATEWAY_URL"
flux secrets set API_KEY                 "YOUR_API_KEY"
flux secrets set PROJECT_ID              "YOUR_PROJECT_ID"
```

---

## Step 4 — Deploy

```bash
flux deploy on_webhook
```

---

## Step 5 — Configure your webhook provider

Point your Stripe / GitHub webhook to:

```
https://YOUR_GATEWAY/on_webhook
```

---

## Tracing a webhook

```bash
# Simulate a webhook call
curl -X POST https://YOUR_GATEWAY/on_webhook \
  -H "Content-Type: application/json" \
  -d '{
    "source": "github",
    "signature": "sha256=...",
    "raw": "{\"type\":\"push\",\"ref\":\"refs/heads/main\"}"
  }' -D - | grep x-request-id

flux trace <request-id>
```

---

## Retry pattern for failed events

Query events that weren't processed (via a cron function):

```typescript
const failed = await flux.db.webhook_events
  .where("processed", "eq", false)
  .where("created_at", "lt", new Date(Date.now() - 60_000).toISOString())
  .limit(50)
  .execute();

for (const event of failed) {
  await handleEvent(event.source, event.payload as any, { flux, ctx });
}
```
