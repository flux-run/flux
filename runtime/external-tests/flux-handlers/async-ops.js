/**
 * async-ops.js — Flux integration test handler
 *
 * Tests async/await and Promise combinator behaviour inside the Deno isolate.
 *
 * Note: The flux-runtime patches setTimeout for replay determinism.
 * Real-time delays via setTimeout are not guaranteed to wait, so timing
 * assertions are intentionally excluded from this suite.
 *
 * Routes:
 *   GET  /async/await         – sequential await
 *   GET  /async/promise-all   – Promise.all
 *   GET  /async/promise-race  – Promise.race (fastest wins)
 *   POST /async/pipeline      – awaits each field transformation in order
 *   GET  /async/microtask     – micro-task ordering via resolved promises
 */

Deno.serve(async (req) => {
  const url = new URL(req.url);

  switch (url.pathname) {
    case "/async/await": {
      const a = await Promise.resolve(1);
      const b = await Promise.resolve(2);
      return Response.json({ result: a + b });
    }

    case "/async/promise-all": {
      const [x, y, z] = await Promise.all([
        Promise.resolve("alpha"),
        Promise.resolve("beta"),
        Promise.resolve("gamma"),
      ]);
      return Response.json({ results: [x, y, z] });
    }

    case "/async/promise-race": {
      const fastest = await Promise.race([
        new Promise((r) => Promise.resolve().then(() => r("slow"))),
        Promise.resolve("fast"),
      ]);
      return Response.json({ winner: fastest });
    }

    case "/async/microtask": {
      const order = [];
      await Promise.resolve().then(() => order.push("microtask-1"));
      await Promise.resolve().then(() => order.push("microtask-2"));
      return Response.json({ order });
    }

    case "/async/pipeline": {
      let body;
      try {
        body = await req.json();
      } catch {
        return new Response(JSON.stringify({ error: "invalid JSON" }), {
          status: 400,
          headers: { "content-type": "application/json" },
        });
      }
      const { value = 0 } = body;
      const step1 = await Promise.resolve(value * 2);
      const step2 = await Promise.resolve(step1 + 10);
      return Response.json({ step1, step2 });
    }

    default:
      return new Response(JSON.stringify({ error: "not found" }), {
        status: 404,
        headers: { "content-type": "application/json" },
      });
  }
});
