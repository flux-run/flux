# Example — AI Backend

Cache-aware AI classification with full execution tracing. Every API call
to the LLM is recorded — no more guessing why a classification returned a
surprise result.

---

## What you'll build

A text classification endpoint that:
1. Hashes the input to check for a cached result in `ctx.db`
2. Calls OpenAI if no cache hit
3. Stores the result for future lookups
4. Traces everything — prompt, model, latency, cache hit/miss

---

## Step 1 — Create the project

```bash
flux init ai-backend && cd ai-backend
```

---

## Step 2 — Define the schema

`schemas/classifications.sql`:

```sql
CREATE TABLE IF NOT EXISTS classifications (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  input_hash   TEXT NOT NULL UNIQUE,
  input_text   TEXT NOT NULL,
  category     TEXT NOT NULL,
  confidence   REAL NOT NULL,
  model        TEXT NOT NULL,
  prompt_tokens INT,
  total_tokens  INT,
  latency_ms   INT,
  created_at   TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_classifications_hash ON classifications(input_hash);
```

```bash
flux db push
```

---

## Step 3 — Set secrets

```bash
flux secrets set OPENAI_API_KEY sk-...
```

---

## Step 4 — Write the function

`functions/classify/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createHash } from "node:crypto";

const CATEGORIES = ["bug", "feature", "question", "docs", "spam"] as const;

export default defineFunction({
  name: "classify",
  input: z.object({ text: z.string().min(1).max(5000) }),
  output: z.object({
    category:   z.enum(CATEGORIES),
    confidence: z.number(),
    cached:     z.boolean(),
  }),

  handler: async ({ input, ctx }) => {
    const hash = createHash("sha256").update(input.text).digest("hex");

    // 1. Check cache
    const cached = await ctx.db.classifications.findOne({ input_hash: hash });
    if (cached) {
      ctx.log.info(`Cache hit for ${hash.slice(0, 8)}`);
      return { category: cached.category, confidence: cached.confidence, cached: true };
    }

    // 2. Call OpenAI
    const apiKey = ctx.secrets.get("OPENAI_API_KEY");
    const start = Date.now();

    const response = await fetch("https://api.openai.com/v1/chat/completions", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${apiKey}`,
      },
      body: JSON.stringify({
        model: "gpt-4o-mini",
        temperature: 0,
        messages: [
          {
            role: "system",
            content: `Classify the following text into exactly one category: ${CATEGORIES.join(", ")}. Respond with JSON: {"category": "...", "confidence": 0.0-1.0}`,
          },
          { role: "user", content: input.text },
        ],
        response_format: { type: "json_object" },
      }),
    });

    const latencyMs = Date.now() - start;

    if (!response.ok) {
      return ctx.error(502, "OPENAI_ERROR", `OpenAI returned ${response.status}`);
    }

    const data = await response.json();
    const usage = data.usage;
    const result = JSON.parse(data.choices[0].message.content);

    ctx.log.info(`Classified as ${result.category} (${result.confidence}) in ${latencyMs}ms`);

    // 3. Cache the result
    await ctx.db.classifications.insert({
      input_hash:    hash,
      input_text:    input.text,
      category:      result.category,
      confidence:    result.confidence,
      model:         "gpt-4o-mini",
      prompt_tokens: usage?.prompt_tokens,
      total_tokens:  usage?.total_tokens,
      latency_ms:    latencyMs,
    });

    return { category: result.category, confidence: result.confidence, cached: false };
  },
});
```

---

## Step 5 — Start and test

```bash
flux dev
```

```bash
# First call — hits OpenAI
curl -X POST http://localhost:4000/classify \
  -H "Content-Type: application/json" \
  -d '{"text": "The login page throws a 500 error when I click submit"}'

# {"category":"bug","confidence":0.95,"cached":false}

# Second call — same text, cache hit
curl -X POST http://localhost:4000/classify \
  -H "Content-Type: application/json" \
  -d '{"text": "The login page throws a 500 error when I click submit"}'

# {"category":"bug","confidence":0.95,"cached":true}
```

---

## Step 6 — Trace the execution

```bash
flux trace <request-id>
```

Cache miss:
```
Trace e4b2a1c3-...  830ms end-to-end

  10:22:01.000  +0ms    ▶ [gateway/classify]      route matched
  10:22:01.003  +3ms    ▶ [runtime/classify]      executing function
  10:22:01.005  +2ms    · [db/classifications]    SELECT by input_hash (2ms) → 0 rows
  10:22:01.008  +3ms    · [external/openai]       POST /v1/chat/completions
  10:22:01.820  +812ms  · [external/openai]       200 OK (812ms)
  10:22:01.825  +5ms    · [db/classifications]    INSERT 1 row (3ms)
  10:22:01.830  +5ms    ■ [runtime/classify]      completed (827ms)

  State changes:
    classifications  INSERT  id=f1a2b3  category="bug"  model="gpt-4o-mini"
  External calls:
    POST api.openai.com  812ms  200  prompt_tokens=85  total_tokens=98
```

Cache hit:
```
Trace 8a1b2c3d-...  7ms end-to-end

  10:22:05.000  +0ms   ▶ [gateway/classify]      route matched
  10:22:05.003  +3ms   ▶ [runtime/classify]      executing function
  10:22:05.005  +2ms   · [db/classifications]    SELECT by input_hash (2ms) → 1 row
  10:22:05.007  +2ms   ■ [runtime/classify]      completed (4ms)

  State changes: none
  External calls: none
```

The cached path had zero external calls and no database mutations. `flux why`
shows the cache hit: "OpenAI call skipped — cache hit on hash `a1b2c3d4...`."

---

## Cost-aware debugging

When your AI bill spikes, `flux trace` tells you exactly what happened:

```bash
# Find the most expensive executions (by token count)
flux trace list --sort=external_calls --limit=20

# See what prompts were sent
flux trace <id> --detail
```

Every external HTTP call — including payload sizes and response codes — is part
of the execution record. No more guessing what prompt caused the spike.
