# Example — AI Backend

Call an external LLM (OpenAI), cache the result via Flux's edge cache, and
log everything end-to-end — without managing any infrastructure.

---

## What you'll build

A `classify_text` function that:

1. Takes a text string as input
2. Classifies it using GPT-4o-mini (sentiment + category)
3. Caches results by content hash to avoid redundant API calls
4. Returns structured output with the classification and metadata

---

## Schema

Create a `classifications` table to persist results:

| Column | Type |
|---|---|
| `id` | `uuid` (primary key, auto) |
| `content_hash` | `text` — SHA-256 of the input |
| `input` | `text` |
| `sentiment` | `text` — `"positive"`, `"negative"`, `"neutral"` |
| `category` | `text` — e.g. `"support"`, `"billing"`, `"feedback"` |
| `model` | `text` — model used |
| `tokens` | `integer` — total tokens consumed |
| `created_at` | `timestamptz` — auto |

---

## The function

Create `classify_text/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";
import { createHash } from "node:crypto";

// Output shape from GPT
const ClassificationSchema = z.object({
  sentiment: z.enum(["positive", "negative", "neutral"]),
  category:  z.enum(["support", "billing", "feedback", "spam", "other"]),
  summary:   z.string().max(200),
});

type Classification = z.infer<typeof ClassificationSchema>;

export default defineFunction({
  name: "classify_text",
  input: z.object({
    text:       z.string().min(1).max(4000),
    categories: z.array(z.string()).optional(),   // custom category list
  }),
  output: ClassificationSchema.extend({
    cached:    z.boolean(),
    tokens:    z.number().int(),
    model:     z.string(),
  }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    // 1. Check cache — same text → same result, no LLM call needed
    const hash = createHash("sha256").update(input.text).digest("hex");
    const [cached] = await flux.db.classifications
      .where("content_hash", "eq", hash)
      .limit(1)
      .execute();

    if (cached) {
      ctx.log(`Cache hit for hash ${hash.slice(0, 8)}`);
      return {
        sentiment: cached.sentiment as Classification["sentiment"],
        category:  cached.category  as Classification["category"],
        summary:   "",
        cached:    true,
        tokens:    cached.tokens,
        model:     cached.model,
      };
    }

    // 2. Call OpenAI
    const apiKey  = ctx.secrets.get("OPENAI_API_KEY");
    if (!apiKey) throw new Error("OPENAI_API_KEY secret not set");

    const model    = "gpt-4o-mini";
    const categories = input.categories ??
      ["support", "billing", "feedback", "spam", "other"];

    ctx.log(`Classifying text with ${model}`);
    const response = await fetch("https://api.openai.com/v1/chat/completions", {
      method:  "POST",
      headers: {
        "Content-Type":  "application/json",
        "Authorization": `Bearer ${apiKey}`,
      },
      body: JSON.stringify({
        model,
        response_format: { type: "json_object" },
        messages: [
          {
            role:    "system",
            content: `Classify the following text. Respond with JSON containing:
- "sentiment": one of "positive", "negative", "neutral"
- "category": one of ${categories.map(c => `"${c}"`).join(", ")}
- "summary": one sentence (max 200 chars) describing the main topic`,
          },
          { role: "user", content: input.text },
        ],
      }),
    });

    if (!response.ok) {
      ctx.log(`OpenAI error: ${response.status}`, "error");
      throw new Error(`OpenAI API error: ${response.status}`);
    }

    const data    = await response.json();
    const usage   = data.usage;
    const content = JSON.parse(data.choices[0].message.content);
    const result  = ClassificationSchema.parse(content);

    ctx.log(
      `Classified: sentiment=${result.sentiment} category=${result.category} tokens=${usage.total_tokens}`,
    );

    // 3. Persist the result for future cache hits
    await flux.db.classifications
      .insert({
        content_hash: hash,
        input:        input.text,
        sentiment:    result.sentiment,
        category:     result.category,
        model,
        tokens:       usage.total_tokens,
      })
      .execute();

    return {
      ...result,
      cached: false,
      tokens: usage.total_tokens,
      model,
    };
  },
});
```

---

## Step 3 — Set secrets

```bash
flux secrets set OPENAI_API_KEY  sk-...
flux secrets set GATEWAY_URL     "https://YOUR_GATEWAY_URL"
flux secrets set API_KEY         "YOUR_API_KEY"
flux secrets set PROJECT_ID      "YOUR_PROJECT_ID"
```

---

## Step 4 — Deploy

```bash
flux deploy classify_text
```

---

## Step 5 — Try it

```bash
# First call — hits OpenAI
flux invoke classify_text --data '{
  "text": "Your product has been amazing, saved us hours every day!"
}'
# → { "sentiment": "positive", "category": "feedback", "cached": false, "tokens": 85, ... }

# Second call with same text — instant cache hit
flux invoke classify_text --data '{
  "text": "Your product has been amazing, saved us hours every day!"
}'
# → { "sentiment": "positive", "category": "feedback", "cached": true, "tokens": 85, ... }
```

---

## Tracing a classification

```bash
flux trace <request-id>
```

Sample trace on a cache miss (shows the full LLM call latency):

```
Trace f8a2c3d1-...  1842ms end-to-end
  ⚠ 1 slow span (>500ms)

  14:05:01.000  +0ms      ▶ [gateway/classify_text]  INFO   route matched
  14:05:01.003  +3ms      · [runtime/classify_text]  INFO   bundle cache hit
  14:05:01.007  +4ms      ▶ [runtime/classify_text]  INFO   executing function
  14:05:01.012  +5ms      · [db/classifications]     INFO   db query on classifications (5ms)
  14:05:02.820  +1808ms   · [function/classify_text] INFO   Classified: sentiment=positive ...
  14:05:02.835  +15ms     · [db/classifications]     INFO   db query on classifications (15ms)
  14:05:02.841  +6ms      ■ [runtime/classify_text]  INFO   execution completed (1834ms)

  7 spans  •  1842ms total

  1 slow span (>500ms):
    +1808ms  function/classify_text  Classified: sentiment=positive category=feedback tokens=85
```

On a cache hit, the trace shows execution completing in < 10 ms with a single
DB query:

```
Trace 9b3e1a22-...  18ms end-to-end

  14:05:10.000  +0ms    ▶ [gateway/classify_text]  INFO   route matched
  14:05:10.002  +2ms    · [runtime/classify_text]  INFO   bundle cache hit
  14:05:10.006  +4ms    ▶ [runtime/classify_text]  INFO   executing function
  14:05:10.009  +3ms    · [db/classifications]     INFO   db query on classifications (3ms)
  14:05:10.013  +4ms    ■ [runtime/classify_text]  INFO   execution completed (7ms)
```

---

## Cost efficiency note

The `classifications` table acts as a semantic cache: identical inputs never
hit the LLM.  For high-traffic workloads, combine with Flux's edge query
cache (30 s TTL by default) for even faster repeated lookups.
