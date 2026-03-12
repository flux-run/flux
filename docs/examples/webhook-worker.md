# Example — Webhook Worker

Receive, verify, and process webhooks from Stripe, GitHub, or any provider.
Full execution recording means every webhook is traceable and debuggable.

---

## What you'll build

A webhook receiver that:
1. Verifies the webhook signature (HMAC-SHA256)
2. Stores the raw event in a `webhook_events` table
3. Dispatches type-specific handling
4. Returns `200 OK` immediately so the sender isn't blocked

---

## Step 1 — Create the project

```bash
flux init webhook-worker && cd webhook-worker
```

---

## Step 2 — Define the schema

`schemas/webhook_events.sql`:

```sql
CREATE TABLE IF NOT EXISTS webhook_events (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  source     TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload    JSONB NOT NULL,
  processed  BOOLEAN NOT NULL DEFAULT false,
  created_at TIMESTAMP DEFAULT NOW()
);
```

```bash
flux db push
```

---

## Step 3 — Set secrets

```bash
flux secrets set STRIPE_WEBHOOK_SECRET  whsec_...
flux secrets set GITHUB_WEBHOOK_SECRET  ghw_...
```

---

## Step 4 — Write the function

`functions/on_webhook/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createHmac, timingSafeEqual } from "node:crypto";

function verifySignature(payload: string, signature: string, secret: string): boolean {
  const expected = createHmac("sha256", secret).update(payload).digest("hex");
  const incoming = signature.replace(/^sha256=/, "");
  try {
    return timingSafeEqual(Buffer.from(expected, "hex"), Buffer.from(incoming, "hex"));
  } catch {
    return false;
  }
}

export default defineFunction({
  name: "on_webhook",
  input: z.object({
    source:    z.string(),          // "stripe" | "github" | ...
    signature: z.string(),          // X-Hub-Signature-256 / Stripe-Signature
    raw:       z.string(),          // raw JSON string body
  }),
  output: z.object({ received: z.boolean() }),

  handler: async ({ input, ctx }) => {
    // 1. Verify signature
    const secret = ctx.secrets.get(`${input.source.toUpperCase()}_WEBHOOK_SECRET`);
    if (!secret) return ctx.error(400, "CONFIG_ERROR", `No webhook secret for ${input.source}`);

    if (!verifySignature(input.raw, input.signature, secret)) {
      return ctx.error(401, "INVALID_SIGNATURE", "Webhook signature verification failed");
    }

    const event = JSON.parse(input.raw);

    // 2. Store the raw event
    const record = await ctx.db.webhook_events.insert({
      source:     input.source,
      event_type: event.type ?? "unknown",
      payload:    event,
      processed:  false,
    });

    ctx.log.info(`Stored webhook ${record.id} (${input.source}/${event.type})`);

    // 3. Dispatch type-specific handling
    try {
      await handleEvent(input.source, event, ctx);
      await ctx.db.webhook_events.update(record.id, { processed: true });
    } catch (err) {
      // Log failure but return 200 — event is stored, can retry later
      ctx.log.error(`Event handling failed: ${err}`);
    }

    return { received: true };
  },
});

async function handleEvent(source: string, event: any, ctx: any) {
  switch (`${source}/${event.type}`) {
    case "stripe/payment_intent.succeeded": {
      const amount = event.data?.object?.amount;
      ctx.log.info(`Payment succeeded: ${amount}`);
      // Update orders, send confirmation, etc.
      break;
    }
    case "github/push": {
      ctx.log.info(`Push to ${event.ref}`);
      // Trigger build, notify Slack, etc.
      break;
    }
    default:
      ctx.log.warn(`Unhandled: ${source}/${event.type}`);
  }
}
```

---

## Step 5 — Start and test

```bash
flux dev
```

```bash
# Simulate a webhook
curl -X POST http://localhost:4000/on_webhook \
  -H "Content-Type: application/json" \
  -d '{
    "source": "github",
    "signature": "sha256=...",
    "raw": "{\"type\":\"push\",\"ref\":\"refs/heads/main\"}"
  }'
```

---

## Step 6 — Trace the webhook

```bash
flux trace <request-id>
```

```
Trace 7f3a1b2c-...  15ms end-to-end

  09:41:02.000  +0ms   ▶ [gateway/on_webhook]    route matched
  09:41:02.003  +3ms   ▶ [runtime/on_webhook]    executing function
  09:41:02.005  +2ms   · [runtime/on_webhook]    signature verified
  09:41:02.010  +5ms   · [db/webhook_events]     INSERT 1 row (5ms)
  09:41:02.013  +3ms   · [db/webhook_events]     UPDATE processed=true (2ms)
  09:41:02.015  +2ms   ■ [runtime/on_webhook]    completed (12ms)

  State changes:
    webhook_events  INSERT  id=a1b2c3d4  source="github"  event_type="push"
    webhook_events  UPDATE  id=a1b2c3d4  processed: false → true
```

If a webhook fails silently, `flux why` shows exactly what happened — including
the unprocessed event in the database.

---

## Why this works with POST-only routing

Webhook providers (Stripe, GitHub, Twilio, etc.) send POST requests. Flux
functions are POST endpoints by design, so inbound webhooks work without any
routing configuration. Point the provider at `https://your-gateway/on_webhook`.
