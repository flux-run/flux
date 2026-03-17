# Bundled Hono Example

This is the intended v1 app flow for Flux: bundle the app first, then run the artifact through the runtime.

Example entry: [examples/hono-hello.ts](../../examples/hono-hello.ts)

```ts
import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", (c) => c.text("hello from hono on flux"));
app.get("/app-health", (c) => c.json({ ok: true }));

Deno.serve(app.fetch);
```

Build and run it:

```bash
flux build examples/hono-hello.ts
flux run examples/hono-hello.ts --listen
```

Then hit it:

```bash
curl http://localhost:3000/hono-hello/
curl http://localhost:3000/hono-hello/app-health
```

Why this shape works well in Flux:

- Hono stays standard user code.
- `Deno.serve(app.fetch)` matches Flux server mode directly.
- `flux build` captures the `npm:hono` graph into the artifact before execution.

This is the recommended path for framework apps in Flux v1. Prefer bundled artifacts over runtime-side package loading.