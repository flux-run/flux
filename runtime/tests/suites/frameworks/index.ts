// Framework compatibility tests
import { TestHarness, assert, assertEquals } from "../../src/harness.js";

export function createFrameworkSuite(): TestHarness {
  const suite = new TestHarness("Frameworks");

  // Express-like framework tests
  suite.test("Express-like routing", () => {
    const routes: Record<string, (req: any) => any> = {};

    const get = (path: string, handler: (req: any) => any) => {
      routes[`GET:${path}`] = handler;
    };

    get("/users", (req) => ({ users: [] }));

    assert("GET:/users" in routes, "Route should be registered");
    const handler = routes["GET:/users"];
    const result = handler({});
    assertEquals(result.users.length, 0, "Handler should return data");
  });

  suite.test("Express-like middleware chain", () => {
    const middlewares: Array<(ctx: any, next: () => void) => void> = [];
    let executionOrder: string[] = [];

    const use = (middleware: (ctx: any, next: () => void) => void) => {
      middlewares.push(middleware);
    };

    use((ctx, next) => {
      executionOrder.push("1");
      next();
    });

    use((ctx, next) => {
      executionOrder.push("2");
      next();
    });

    use((ctx) => {
      executionOrder.push("3");
    });

    const ctx = {};
    let middlewareIndex = 0;
    const next = () => {
      if (middlewareIndex < middlewares.length) {
        const middleware = middlewares[middlewareIndex++];
        middleware(ctx, next);
      }
    };

    next();
    assertEquals(executionOrder.length, 3, "All middlewares should execute");
    assertEquals(executionOrder[0], "1", "Middlewares should execute in order");
  });

  suite.test("Koa-like context", () => {
    const ctx = {
      request: { url: "/api/users", method: "GET" },
      response: { status: 200, body: null as any },
    };

    ctx.response.status = 200;
    ctx.response.body = { users: [] };

    assertEquals(ctx.response.status, 200, "Context status should work");
    assert(Array.isArray(ctx.response.body.users), "Context body should work");
  });

  suite.test("Request body parsing", async () => {
    const json = { name: "John", age: 30 };
    const body = JSON.stringify(json);

    const parsedBody = JSON.parse(body);
    assertEquals(parsedBody.name, "John", "Body parsing should work");
    assertEquals(parsedBody.age, 30, "Body parsing should work");
  });

  suite.test("Response serialization", () => {
    const data = { id: 1, name: "test" };
    const json = JSON.stringify(data);

    assert(json.includes("id"), "Response serialization should work");
    assert(json.includes("name"), "Response serialization should work");
  });

  suite.test("Route parameters", () => {
    const pattern = /^\/users\/(\d+)$/;
    const match = "/users/123".match(pattern);

    assert(match !== null, "Pattern should match");
    assertEquals(match![1], "123", "Should extract parameter");
  });

  suite.test("Query string parsing", () => {
    const url = new URL("http://localhost/api?page=1&limit=10");
    const page = url.searchParams.get("page");
    const limit = url.searchParams.get("limit");

    assertEquals(page, "1", "Query param should be parsed");
    assertEquals(limit, "10", "Query param should be parsed");
  });

  suite.test("Header handling", () => {
    const headers = new Headers();
    headers.set("authorization", "Bearer token123");
    headers.set("content-type", "application/json");

    assertEquals(
      headers.get("authorization"),
      "Bearer token123",
      "Authorization header should work"
    );
    assertEquals(headers.get("content-type"), "application/json", "Content-type should work");
  });

  suite.test("Error handling", () => {
    let errorCaught = false;
    const handler = (ctx: any) => {
      try {
        throw new Error("Route error");
      } catch (e) {
        errorCaught = true;
        ctx.response = { status: 500, body: { error: "Internal Server Error" } };
      }
    };

    const ctx: any = { response: {} };
    handler(ctx);

    assert(errorCaught, "Error should be caught");
    assertEquals(ctx.response.status, 500, "Error status should be set");
  });

  suite.test("Async handler execution", async () => {
    let executed = false;
    const handler = async (ctx: any) => {
      await new Promise((resolve) => setTimeout(resolve, 10));
      executed = true;
      ctx.response = { status: 200, body: { ok: true } };
    };

    const ctx: any = { response: {} };
    await handler(ctx);

    assert(executed, "Async handler should execute");
    assertEquals(ctx.response.status, 200, "Async handler should set response");
  });

  suite.test("Request validation", () => {
    const validate = (data: any, schema: Record<string, string>) => {
      for (const [key, type] of Object.entries(schema)) {
        if (typeof data[key] !== type) {
          throw new Error(`Invalid ${key}: expected ${type}`);
        }
      }
    };

    const data = { name: "John", age: 30 };
    const schema = { name: "string", age: "number" };

    try {
      validate(data, schema);
      assert(true, "Valid data should pass");
    } catch (e) {
      assert(false, "Valid data should pass");
    }
  });

  suite.test("HTTP method detection", () => {
    const request = { method: "POST" };
    const isPost = request.method === "POST";
    const isGet = request.method === "GET";

    assert(isPost, "POST detection should work");
    assert(!isGet, "GET detection should work");
  });

  suite.test("Static content serving", () => {
    const serveStatic = (filepath: string) => {
      if (filepath.endsWith(".json")) {
        return { type: "application/json" };
      } else if (filepath.endsWith(".html")) {
        return { type: "text/html" };
      }
      return { type: "text/plain" };
    };

    assertEquals(serveStatic("data.json").type, "application/json", "JSON content type");
    assertEquals(serveStatic("index.html").type, "text/html", "HTML content type");
  });

  suite.test("Cookie handling", () => {
    const cookies: Record<string, string> = {};

    const setCookie = (name: string, value: string) => {
      cookies[name] = value;
    };

    const getCookie = (name: string) => {
      return cookies[name];
    };

    setCookie("session", "abc123");
    assertEquals(getCookie("session"), "abc123", "Cookie should be set and retrieved");
  });

  suite.test("CORS handling", () => {
    const corsHeaders = (origin: string) => {
      return {
        "access-control-allow-origin": origin,
        "access-control-allow-methods": "GET, POST, PUT, DELETE",
      };
    };

    const headers = corsHeaders("http://example.com");
    assertEquals(headers["access-control-allow-origin"], "http://example.com", "CORS origin");
  });

  suite.test("Rate limiting", () => {
    const requests: { [key: string]: number[] } = {};

    const checkRateLimit = (clientId: string, limit: number, windowMs: number) => {
      const now = Date.now();
      if (!requests[clientId]) {
        requests[clientId] = [];
      }

      requests[clientId] = requests[clientId].filter((time) => now - time < windowMs);

      if (requests[clientId].length >= limit) {
        return false;
      }

      requests[clientId].push(now);
      return true;
    };

    const clientId = "user123";
    const result1 = checkRateLimit(clientId, 3, 1000);
    const result2 = checkRateLimit(clientId, 3, 1000);
    const result3 = checkRateLimit(clientId, 3, 1000);
    const result4 = checkRateLimit(clientId, 3, 1000);

    assert(result1 && result2 && result3, "First 3 requests should pass");
    assert(!result4, "4th request should be rate limited");
  });

  suite.test("Dependency injection", () => {
    const container: { [key: string]: any } = {};

    const register = (name: string, factory: () => any) => {
      container[name] = factory;
    };

    const resolve = (name: string) => {
      const factory = container[name];
      return factory ? factory() : null;
    };

    register("db", () => ({ query: () => [] }));
    const db = resolve("db");

    assert(db !== null, "DI should resolve dependency");
    assert(typeof db.query === "function", "Dependency should have expected methods");
  });

  return suite;
}
