/**
 * run-flux-integration.ts
 *
 * Integration tests that compile and run the actual flux-runtime binary,
 * then send real HTTP requests to validate runtime behaviour.
 *
 * Unlike the internal test suites (trust / compat / replay / modules), which
 * run inside a Node.js process, these tests prove that the *Flux binary* is
 * working correctly end-to-end.
 *
 * Usage
 * -----
 *   npm run test:integration              # build binary (if missing) + test
 *   npm run test:integration -- --skip-build   # skip cargo build
 *   npm run test:integration -- --suite echo   # run one suite only
 *
 * Output
 * ------
 *   runtime/reports/flux-integration.json
 */

import { performance }   from "node:perf_hooks";
import { spawnSync }     from "node:child_process";
import { existsSync, readFileSync }  from "node:fs";
import { resolve }       from "node:path";
import { dirname }       from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";
import {
  ensureBinary,
  buildArtifact,
  startRuntime,
  startServer,
  FLUX_CLI_BIN,
  postJson,
  get,
  WORKSPACE_ROOT,
  type RuntimeHandle,
} from "./lib/flux-binary.js";
import { TestResult, buildReport, writeReport, printSummary } from "./lib/utils.js";

const __dirname   = dirname(fileURLToPath(import.meta.url));
const HANDLERS_DIR = resolve(__dirname, "../external-tests/flux-handlers");
const EXAMPLES_DIR = resolve(WORKSPACE_ROOT, "examples");
const CRUD_APP_DIR = resolve(EXAMPLES_DIR, "crud_app");
const CRUD_INIT_SQL = resolve(CRUD_APP_DIR, "init.sql");
const DRIZZLE_DIR = resolve(EXAMPLES_DIR, "drizzle");

// Each suite gets its own port in the 3100-3199 range so suites can run
// sequentially without port conflicts when multiple are enabled.
let nextPort = 3100;
function allocatePort() { return nextPort++; }

let nextDatabasePort = 55432;
function allocateDatabasePort() { return nextDatabasePort++; }

let nextServerPort = 51051;
function allocateServerPort() { return nextServerPort++; }

// ---------------------------------------------------------------------------
// Assertion helper
// ---------------------------------------------------------------------------

interface AssertionContext {
  results: TestResult[];
}

interface PostgresHandle {
  containerName: string;
  databaseUrl: string;
  stop(): void;
}

interface PostgresConfig {
  databaseName: string;
  username: string;
  password: string;
  initSql?: string;
}

const crudReplayState = {
  serverUrl: "",
  serviceToken: "",
};

function assert(
  ctx: AssertionContext,
  name: string,
  fn: () => boolean | string,
): void {
  const start = performance.now();
  let passed = false;
  let error: string | undefined;
  try {
    const res = fn();
    if (typeof res === "string") {
      passed = false;
      error  = res;
    } else {
      passed = res;
      if (!passed) error = "assertion returned false";
    }
  } catch (e) {
    passed = false;
    error  = (e as Error).message;
  }
  ctx.results.push({
    name,
    passed,
    skipped: false,
    error,
    duration: performance.now() - start,
  });
}

function runCheckedCommand(
  command: string,
  args: string[],
  opts: { cwd?: string; env?: NodeJS.ProcessEnv; input?: string } = {},
): string {
  const result = spawnSync(command, args, {
    cwd: opts.cwd ?? WORKSPACE_ROOT,
    env: { ...process.env, ...opts.env },
    encoding: "utf8",
    input: opts.input,
  });

  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (exit ${result.status})\n${result.stderr ?? result.stdout ?? ""}`,
    );
  }

  return result.stdout ?? "";
}

async function waitForPostgres(containerName: string, timeoutMs = 15_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = spawnSync("docker", ["exec", containerName, "pg_isready", "-U", "postgres", "-d", "postgres"], {
      cwd: WORKSPACE_ROOT,
      encoding: "utf8",
    });

    if (result.status === 0) {
      return;
    }

    await sleep(250);
  }

  throw new Error(`postgres container ${containerName} did not become ready within ${timeoutMs}ms`);
}

async function startPostgres(hostPort: number, config: PostgresConfig): Promise<PostgresHandle> {
  const containerName = `flux-int-${config.databaseName}-${hostPort}`;
  const databaseUrl = `postgres://${config.username}:${config.password}@127.0.0.1:${hostPort}/${config.databaseName}`;

  runCheckedCommand("docker", [
    "run",
    "--rm",
    "-d",
    "--name",
    containerName,
    "-e",
    `POSTGRES_USER=${config.username}`,
    "-e",
    `POSTGRES_PASSWORD=${config.password}`,
    "-e",
    `POSTGRES_DB=${config.databaseName}`,
    "-p",
    `${hostPort}:5432`,
    "postgres:17-alpine",
  ]);

  try {
    await waitForPostgres(containerName);
    if (config.initSql) {
      runCheckedCommand(
        "docker",
        ["exec", "-i", containerName, "psql", "-U", config.username, "-d", config.databaseName, "-v", "ON_ERROR_STOP=1"],
        { input: config.initSql },
      );
    }
  } catch (error) {
    runCheckedCommand("docker", ["rm", "-f", containerName]);
    throw error;
  }

  return {
    containerName,
    databaseUrl,
    stop() {
      runCheckedCommand("docker", ["rm", "-f", containerName]);
    },
  };
}

async function startCrudPostgres(hostPort: number): Promise<PostgresHandle> {
  return startPostgres(hostPort, {
    databaseName: "crud_app",
    username: "postgres",
    password: "postgres",
    initSql: readFileSync(CRUD_INIT_SQL, "utf8"),
  });
}

async function startDrizzlePostgres(hostPort: number): Promise<PostgresHandle> {
  return startPostgres(hostPort, {
    databaseName: "madmonkey",
    username: "admin",
    password: "password123",
  });
}

function extractReplayOutput(stdout: string): string {
  const match = stdout.match(/^\s*output\s+(.+)$/m);
  if (!match) {
    throw new Error(`could not find replay output in CLI output\n${stdout}`);
  }
  return match[1];
}

function stripAnsi(value: string): string {
  return value.replace(/\x1b\[[0-9;]*m/g, "");
}

function extractCommandOutput(stdout: string): string {
  const cleaned = stripAnsi(stdout).trimEnd();
  const objectStart = cleaned.lastIndexOf("\n{");
  const arrayStart = cleaned.lastIndexOf("\n[");
  const start = Math.max(objectStart, arrayStart);

  if (start === -1) {
    throw new Error(`could not find command output in CLI output\n${stdout}`);
  }

  return cleaned.slice(start + 1).trim();
}

function extractExecutionId(stdout: string): string {
  const match = stripAnsi(stdout).match(/^\s*execution_id:\s*([0-9a-f-]+)\s*$/mi);
  if (!match) {
    throw new Error(`could not find execution_id in CLI output\n${stdout}`);
  }

  return match[1];
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableJson(item)).join(",")}]`;
  }

  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, item]) => `${JSON.stringify(key)}:${stableJson(item)}`);
    return `{${entries.join(",")}}`;
  }

  return JSON.stringify(value);
}

function ensureDrizzleExampleDependencies(): void {
  const drizzleOrmPackage = resolve(DRIZZLE_DIR, "node_modules", "drizzle-orm", "package.json");
  if (existsSync(drizzleOrmPackage)) {
    return;
  }

  runCheckedCommand("npm", ["ci"], { cwd: DRIZZLE_DIR });
}

// ---------------------------------------------------------------------------
// Suite runner wrapper
// ---------------------------------------------------------------------------

interface Suite {
  name:    string;
  handler?: string;   // filename inside HANDLERS_DIR
  handlerBaseDir?: "handlers" | "examples";
  start?: (entry: string, port: number) => Promise<RuntimeHandle>;
  run?: (baseUrl: string, ctx: AssertionContext) => Promise<void>;
  execute?: (ctx: AssertionContext) => Promise<void>;
}

async function runSuite(suite: Suite): Promise<{ passed: number; failed: number; results: TestResult[] }> {
  const ctx: AssertionContext = { results: [] };

  let runtime: RuntimeHandle | null = null;
  try {
    if (suite.execute) {
      await suite.execute(ctx);
    } else {
      if (!suite.handler || !suite.run) {
        throw new Error(`suite ${suite.name} is missing a handler or run function`);
      }

      const port = allocatePort();
      const entryBaseDir = suite.handlerBaseDir === "examples" ? EXAMPLES_DIR : HANDLERS_DIR;
      const entry = resolve(entryBaseDir, suite.handler);

      buildArtifact(entry, { quiet: true });
      runtime = suite.start ? await suite.start(entry, port) : await startRuntime(entry, port);
      await suite.run(runtime.baseUrl, ctx);
    }
  } catch (err) {
    ctx.results.push({
      name:    `[suite startup] ${suite.name}`,
      passed:  false,
      skipped: false,
      error:   (err as Error).message,
      duration: 0,
    });
  } finally {
    await runtime?.stop();
  }

  const passed = ctx.results.filter((r) => r.passed).length;
  const failed = ctx.results.filter((r) => !r.passed).length;
  return { passed, failed, results: ctx.results };
}

// ---------------------------------------------------------------------------
// Suite definitions
// ---------------------------------------------------------------------------

const SUITES: Suite[] = [

  // ── 1. Echo ─────────────────────────────────────────────────────────────
  {
    name:    "echo",
    handler: "echo.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/ping");
        assert(ctx, "GET /ping → 200", () => r.status === 200);
        assert(ctx, "GET /ping body has ok:true", () => (r.body as any)?.ok === true);
      }
      {
        const payload = { hello: "world", num: 42 };
        const r = await postJson(baseUrl, "/echo", payload);
        assert(ctx, "POST /echo → 200", () => r.status === 200);
        assert(ctx, "POST /echo reflects string field", () => (r.body as any)?.hello === "world");
        assert(ctx, "POST /echo reflects numeric field", () => (r.body as any)?.num === 42);
      }
      {
        const r = await postJson(baseUrl, "/echo/upper", { greeting: "hello", count: 7 });
        assert(ctx, "POST /echo/upper → 200", () => r.status === 200);
        assert(ctx, "POST /echo/upper uppercases strings", () => (r.body as any)?.greeting === "HELLO");
        assert(ctx, "POST /echo/upper passes through numbers", () => (r.body as any)?.count === 7);
      }
      {
        const res = await fetch(`${baseUrl}/echo`, {
          method:  "POST",
          headers: { "content-type": "application/json" },
          body:    "not json {{",
        });
        assert(ctx, "POST /echo with bad JSON → 400", () => res.status === 400);
      }
    },
  },

  // ── 2. JSON types ────────────────────────────────────────────────────────
  {
    name:    "json-types",
    handler: "json-types.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/types/null");
        assert(ctx, "GET /types/null → value is null", () => (r.body as any)?.value === null);
      }
      {
        const r = await get(baseUrl, "/types/bool");
        assert(ctx, "GET /types/bool → value is true", () => (r.body as any)?.value === true);
      }
      {
        const r = await get(baseUrl, "/types/number");
        assert(ctx, "GET /types/number → integer 42", () => (r.body as any)?.value === 42);
        assert(ctx, "GET /types/number → float 3.14", () => Math.abs((r.body as any)?.float - 3.14) < 0.001);
      }
      {
        const r = await get(baseUrl, "/types/string");
        assert(ctx, "GET /types/string → 'hello flux'", () => (r.body as any)?.value === "hello flux");
      }
      {
        const r = await get(baseUrl, "/types/array");
        const v = (r.body as any)?.value;
        assert(ctx, "GET /types/array → array length 4", () => Array.isArray(v) && v.length === 4);
        assert(ctx, "GET /types/array → element types", () => v[0] === 1 && v[1] === "two" && v[2] === true && v[3] === null);
      }
      {
        const r = await get(baseUrl, "/types/nested");
        const o = (r.body as any)?.outer;
        assert(ctx, "GET /types/nested → deep field", () => o?.inner?.deep === "yes");
        assert(ctx, "GET /types/nested → nested array", () => Array.isArray(o?.arr));
      }
      {
        const r = await get(baseUrl, "/types/all");
        const b = r.body as any;
        assert(ctx, "GET /types/all → null field",   () => b?.null === null);
        assert(ctx, "GET /types/all → bool false",   () => b?.bool === false);
        assert(ctx, "GET /types/all → negative int", () => b?.integer === -7);
        assert(ctx, "GET /types/all → UTF-8 string", () => typeof b?.string === "string" && b.string.includes("🎉"));
      }
      {
        const r = await get(baseUrl, "/types/missing");
        assert(ctx, "GET /types/missing → 404", () => r.status === 404);
      }
    },
  },

  // ── 3. Web APIs ──────────────────────────────────────────────────────────
  {
    name:    "web-apis",
    handler: "web-apis.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/web/uuid");
        assert(ctx, "GET /web/uuid → valid RFC-4122 UUID",
          () => /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
            .test((r.body as any)?.id));
        assert(ctx, "GET /web/uuid → valid:true", () => (r.body as any)?.valid === true);
      }
      {
        const r = await get(baseUrl, "/web/date");
        assert(ctx, "GET /web/date → timestamp is number", () => typeof (r.body as any)?.timestamp === "number");
        assert(ctx, "GET /web/date → ISO string", () => typeof (r.body as any)?.iso === "string");
      }
      {
        const r = await get(baseUrl, "/web/url");
        const b = r.body as any;
        assert(ctx, "GET /web/url → host", () => b?.host === "example.com");
        assert(ctx, "GET /web/url → pathname", () => b?.pathname === "/path");
        assert(ctx, "GET /web/url → foo param", () => b?.foo === "1");
      }
      {
        const r = await get(baseUrl, "/web/url-build");
        const b = r.body as any;
        assert(ctx, "GET /web/url-build → href contains path", () => typeof b?.href === "string" && b.href.includes("/v1/users"));
        assert(ctx, "GET /web/url-build → page param", () => b?.page === "2");
        assert(ctx, "GET /web/url-build → pathname", () => b?.path === "/v1/users");
      }
      {
        const r = await get(baseUrl, "/web/url-search-params");
        const b = r.body as any;
        assert(ctx, "GET /web/url-search-params → repeated params", () => Array.isArray(b?.tags) && b.tags.length === 2 && b.tags[0] === "alpha" && b.tags[1] === "beta");
        assert(ctx, "GET /web/url-search-params → plus decodes to space", () => b?.space === "hello world");
        assert(ctx, "GET /web/url-search-params → append/set semantics", () => b?.extra === "42" && b?.single === "value" && b?.hasExtra === true);
        assert(ctx, "GET /web/url-search-params → serialized output", () => typeof b?.text === "string" && b.text.includes("tag=alpha") && b.text.includes("extra=42"));
      }
      {
        const res = await fetch(`${baseUrl}/web/headers`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-custom": "MiXeD",
          },
          body: JSON.stringify({ ok: true }),
        });
        const b = await res.json() as any;
        assert(ctx, "POST /web/headers → 202", () => res.status === 202);
        assert(ctx, "POST /web/headers → inbound header visible", () => b?.inbound === "MiXeD");
        assert(ctx, "POST /web/headers → case-insensitive lookup", () => b?.caseInsensitive === "MiXeD");
        assert(ctx, "POST /web/headers → request content-type visible", () => b?.hasJson === true);
        assert(ctx, "POST /web/headers → appended response header preserved", () => res.headers.get("x-one") === "alpha, beta");
        assert(ctx, "POST /web/headers → response header set", () => res.headers.get("x-two") === "gamma");
      }
      {
        const res = await fetch(`${baseUrl}/web/request-info?foo=bar`, {
          method: "POST",
          headers: {
            "content-type": "text/plain",
            "x-custom": "request-header",
          },
          body: "payload-body",
        });
        const b = await res.json() as any;
        assert(ctx, "POST /web/request-info → Request instance", () => b?.isRequest === true);
        assert(ctx, "POST /web/request-info → method preserved", () => b?.method === "POST");
        assert(ctx, "POST /web/request-info → query visible", () => b?.query === "bar");
        assert(ctx, "POST /web/request-info → header visible", () => b?.header === "request-header");
        assert(ctx, "POST /web/request-info → body readable", () => b?.body === "payload-body");
      }
      {
        const r = await get(baseUrl, "/web/request-construct");
        const b = r.body as any;
        assert(ctx, "GET /web/request-construct → Request constructor available", () => b?.isRequest === true);
        assert(ctx, "GET /web/request-construct → method set", () => b?.method === "POST");
        assert(ctx, "GET /web/request-construct → URL host/query set", () => b?.host === "api.example.com" && b?.query === "bar");
        assert(ctx, "GET /web/request-construct → headers readable", () => b?.contentType === "text/plain" && b?.extra === "demo");
        assert(ctx, "GET /web/request-construct → body readable", () => b?.body === "payload");
      }
      {
        const res = await fetch(`${baseUrl}/web/response`);
        const text = await res.text();
        assert(ctx, "GET /web/response → status preserved", () => res.status === 201);
        assert(ctx, "GET /web/response → header preserved", () => res.headers.get("x-response") === "ok");
        assert(ctx, "GET /web/response → body preserved", () => text === "created");
      }
      {
        const r = await get(baseUrl, "/web/text-encoding");
        const b = r.body as any;
        assert(ctx, "GET /web/text-encoding → decode round-trip", () => b?.decoded === "Flux 日本語");
        assert(ctx, "GET /web/text-encoding → UTF-8 bytes produced", () => typeof b?.byteLength === "number" && b.byteLength > "Flux 日本語".length);
        assert(ctx, "GET /web/text-encoding → byte prefix returned", () => Array.isArray(b?.prefix) && b.prefix.length === 4);
      }
      {
        const r = await get(baseUrl, "/web/math");
        const b = r.body as any;
        assert(ctx, "GET /web/math → random in [0,1)", () => b?.random_in_range === true);
        assert(ctx, "GET /web/math → floor(3.9)=3",   () => b?.floor === 3);
        assert(ctx, "GET /web/math → ceil(3.1)=4",    () => b?.ceil === 4);
        assert(ctx, "GET /web/math → abs(-7)=7",      () => b?.abs === 7);
        assert(ctx, "GET /web/math → min(5,3,8)=3",   () => b?.min === 3);
        assert(ctx, "GET /web/math → max(5,3,8)=8",   () => b?.max === 8);
        assert(ctx, "GET /web/math → 2^10=1024",      () => b?.pow === 1024);
      }
      {
        const r = await get(baseUrl, "/web/json");
        const b = r.body as any;
        assert(ctx, "GET /web/json → JSON round-trip match", () => b?.match === true);
        assert(ctx, "GET /web/json → json field is string", () => typeof b?.json === "string");
      }
    },
  },

  // ── 6. Bundled framework app ───────────────────────────────────────────
  {
    name: "bundled-hono",
    handler: "hono-hello.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      {
        const res = await fetch(`${baseUrl}/`, {
          headers: { host: "localhost" },
        });
        const text = await res.text();
        assert(ctx, "GET / → 200", () => res.status === 200);
        assert(ctx, "GET / → hono text body", () => text === "hello from hono on flux");
      }
      {
        const res = await fetch(`${baseUrl}/app-health`, {
          headers: { host: "localhost" },
        });
        const body = await res.json() as any;
        assert(ctx, "GET /app-health → 200", () => res.status === 200);
        assert(ctx, "GET /app-health → json ok:true", () => body?.ok === true);
      }
    },
  },

  // ── 7. CRUD replay ──────────────────────────────────────────────────────
  {
    name: "crud-replay",
    handler: "crud_app/main_flux.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startCrudPostgres(databasePort);
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        crudReplayState.serverUrl = server.url;
        crudReplayState.serviceToken = serviceToken;
        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1",
          },
        });

        return {
          ...runtime,
          async stop() {
            try {
              await runtime.stop();
            } finally {
              try {
                await server.stop();
              } finally {
                postgres.stop();
                crudReplayState.serverUrl = "";
                crudReplayState.serviceToken = "";
              }
            }
          },
        };
      } catch (error) {
        crudReplayState.serverUrl = "";
        crudReplayState.serviceToken = "";
        await server.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const initialList = await get(baseUrl, "/todos");
      const initialTodos = initialList.body as any[];
      assert(ctx, "GET /todos before create → 200", () => initialList.status === 200);
      assert(ctx, "GET /todos before create → empty", () => Array.isArray(initialTodos) && initialTodos.length === 0);

      const createRes = await fetch(`${baseUrl}/todos`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          title: "Ship Flux",
          description: "Replay integration",
        }),
      });
      const createBody = await createRes.json() as Record<string, unknown>;
      const executionId = createRes.headers.get("x-flux-execution-id");

      assert(ctx, "POST /todos → 201", () => createRes.status === 201);
      assert(ctx, "POST /todos → execution id header", () => typeof executionId === "string" && executionId.length > 0);
      assert(ctx, "POST /todos → title persisted", () => createBody.title === "Ship Flux");
      assert(ctx, "POST /todos → completed false", () => createBody.completed === false);

      const listAfterCreate = await get(baseUrl, "/todos");
      const todosAfterCreate = listAfterCreate.body as any[];
      assert(ctx, "GET /todos after create → one row", () => Array.isArray(todosAfterCreate) && todosAfterCreate.length === 1);

      const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
        "replay",
        executionId ?? "",
        "--url",
        crudReplayState.serverUrl,
        "--token",
        crudReplayState.serviceToken,
        "--diff",
      ]));

      const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
        net_response?: { body?: string; status?: number };
      };
      const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;

      assert(ctx, "flux replay → ok", () => replayStdout.includes("ok"));
      assert(ctx, "flux replay → same JSON response", () => JSON.stringify(replayBody) === JSON.stringify(createBody));
      assert(ctx, "flux replay → 201 status preserved", () => replayEnvelope.net_response?.status === 201);
      assert(ctx, "flux replay → Postgres step recorded", () => replayStdout.includes("POSTGRES") && replayStdout.includes("(recorded)"));
      assert(ctx, "flux replay → writes suppressed", () => replayStdout.includes("db writes suppressed"));

      const listAfterReplay = await get(baseUrl, "/todos");
      const todosAfterReplay = listAfterReplay.body as any[];
      assert(ctx, "GET /todos after replay → count unchanged", () => Array.isArray(todosAfterReplay) && todosAfterReplay.length === 1);
    },
  },

  // ── 8. Drizzle examples ────────────────────────────────────────────────
  {
    name: "drizzle-crud",
    async execute(ctx) {
      ensureDrizzleExampleDependencies();

      const databasePort = allocateDatabasePort();
      const postgres = await startDrizzlePostgres(databasePort);

      try {
        const stdout = runCheckedCommand(
          FLUX_CLI_BIN,
          [
            "run",
            "--input",
            JSON.stringify({ input: { connectionString: postgres.databaseUrl } }),
            resolve(DRIZZLE_DIR, "crud.ts"),
          ],
          {
            cwd: WORKSPACE_ROOT,
            env: { FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1" },
          },
        );

        const payload = JSON.parse(extractCommandOutput(stdout)) as {
          inserted?: { id?: number; title?: string; state?: string };
          selected?: { id?: number; title?: string; state?: string };
          updated?: { id?: number; title?: string; state?: string };
        };

        assert(ctx, "flux run examples/drizzle/crud.ts → inserted row", () => payload.inserted?.title === "ship flux" && payload.inserted?.state === "new");
        assert(ctx, "flux run examples/drizzle/crud.ts → selected row", () => payload.selected?.id === payload.inserted?.id && payload.selected?.state === "new");
        assert(ctx, "flux run examples/drizzle/crud.ts → updated row", () => payload.updated?.id === payload.inserted?.id && payload.updated?.state === "done");
      } finally {
        postgres.stop();
      }
    },
  },
  {
    name: "drizzle-transaction",
    async execute(ctx) {
      ensureDrizzleExampleDependencies();

      const databasePort = allocateDatabasePort();
      const postgres = await startDrizzlePostgres(databasePort);

      try {
        const stdout = runCheckedCommand(
          FLUX_CLI_BIN,
          [
            "run",
            "--input",
            JSON.stringify({ input: { connectionString: postgres.databaseUrl } }),
            resolve(DRIZZLE_DIR, "transaction.ts"),
          ],
          {
            cwd: WORKSPACE_ROOT,
            env: { FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1" },
          },
        );

        const payload = JSON.parse(extractCommandOutput(stdout)) as {
          txResult?: {
            inserted?: { id?: number; name?: string; status?: string };
            selected?: { id?: number; name?: string; status?: string };
            updated?: { id?: number; name?: string; status?: string };
          };
          finalRows?: Array<{ id?: number; name?: string; status?: string }>;
        };

        assert(ctx, "flux run examples/drizzle/transaction.ts → inserted row", () => payload.txResult?.inserted?.name === "replay-check" && payload.txResult?.inserted?.status === "queued");
        assert(ctx, "flux run examples/drizzle/transaction.ts → selected row", () => payload.txResult?.selected?.id === payload.txResult?.inserted?.id && payload.txResult?.selected?.status === "queued");
        assert(ctx, "flux run examples/drizzle/transaction.ts → updated row", () => payload.txResult?.updated?.id === payload.txResult?.inserted?.id && payload.txResult?.updated?.status === "running");
        assert(ctx, "flux run examples/drizzle/transaction.ts → final persisted row", () => Array.isArray(payload.finalRows) && payload.finalRows.length === 1 && payload.finalRows[0]?.status === "running");
      } finally {
        postgres.stop();
      }
    },
  },
  {
    name: "drizzle-replay",
    async execute(ctx) {
      ensureDrizzleExampleDependencies();

      const databasePort = allocateDatabasePort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startDrizzlePostgres(databasePort);
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        const stdout = runCheckedCommand(
          FLUX_CLI_BIN,
          [
            "run",
            "--url",
            server.url,
            "--input",
            JSON.stringify({ input: { connectionString: postgres.databaseUrl } }),
            resolve(DRIZZLE_DIR, "crud.ts"),
          ],
          {
            cwd: WORKSPACE_ROOT,
            env: {
              FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1",
              FLUX_SERVICE_TOKEN: serviceToken,
            },
          },
        );

        const executionId = extractExecutionId(stdout);
        const payload = JSON.parse(extractCommandOutput(stdout)) as {
          inserted?: { id?: number; title?: string; state?: string };
          selected?: { id?: number; title?: string; state?: string };
          updated?: { id?: number; title?: string; state?: string };
        };

        assert(ctx, "flux run examples/drizzle/crud.ts with recording → execution id emitted", () => executionId.length > 0);
        assert(ctx, "flux run examples/drizzle/crud.ts with recording → output captured", () => payload.updated?.state === "done");

        const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "replay",
          executionId,
          "--url",
          server.url,
          "--token",
          serviceToken,
          "--diff",
        ]));

        const replayPayload = JSON.parse(extractReplayOutput(replayStdout)) as {
          inserted?: { id?: number; title?: string; state?: string };
          selected?: { id?: number; title?: string; state?: string };
          updated?: { id?: number; title?: string; state?: string };
        };

        assert(ctx, "flux replay for drizzle CRUD → ok", () => replayStdout.includes("ok"));
        assert(ctx, "flux replay for drizzle CRUD → same JSON output", () => stableJson(replayPayload) === stableJson(payload));
        assert(ctx, "flux replay for drizzle CRUD → Postgres recorded", () => replayStdout.includes("POSTGRES") && replayStdout.includes("(recorded)"));
        assert(ctx, "flux replay for drizzle CRUD → writes suppressed", () => replayStdout.includes("db writes suppressed"));
      } finally {
        await server.stop();
        postgres.stop();
      }
    },
  },

  // ── 4. Async ops ─────────────────────────────────────────────────────────
  {
    name:    "async-ops",
    handler: "async-ops.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/async/await");
        assert(ctx, "GET /async/await → result 3", () => (r.body as any)?.result === 3);
      }
      {
        const r = await get(baseUrl, "/async/promise-all");
        const results = (r.body as any)?.results;
        assert(ctx, "GET /async/promise-all → 3 items", () => Array.isArray(results) && results.length === 3);
        assert(ctx, "GET /async/promise-all → correct values",
          () => results[0] === "alpha" && results[1] === "beta" && results[2] === "gamma");
      }
      {
        const r = await get(baseUrl, "/async/promise-race");
        assert(ctx, "GET /async/promise-race → fast wins", () => (r.body as any)?.winner === "fast");
      }
      {
        const r = await get(baseUrl, "/async/microtask");
        const order = (r.body as any)?.order;
        assert(ctx, "GET /async/microtask → 2 items", () => Array.isArray(order) && order.length === 2);
        assert(ctx, "GET /async/microtask → ordering", () => order[0] === "microtask-1" && order[1] === "microtask-2");
      }
      {
        const r = await postJson(baseUrl, "/async/pipeline", { value: 5 });
        assert(ctx, "POST /async/pipeline → step1 = 10", () => (r.body as any)?.step1 === 10);
        assert(ctx, "POST /async/pipeline → step2 = 20", () => (r.body as any)?.step2 === 20);
      }
    },
  },

  // ── 5. Error handling ────────────────────────────────────────────────────
  {
    name:    "error-handling",
    handler: "error-handling.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/error/not-found");
        assert(ctx, "GET /error/not-found → 404", () => r.status === 404);
        assert(ctx, "GET /error/not-found → error field", () => typeof (r.body as any)?.error === "string");
      }
      {
        const r = await get(baseUrl, "/error/bad-request");
        assert(ctx, "GET /error/bad-request → 400", () => r.status === 400);
      }
      {
        // Unhandled sync throw — runtime should return a 5xx
        const r = await get(baseUrl, "/error/sync-throw");
        assert(ctx, "GET /error/sync-throw → 5xx", () => r.status >= 500);
      }
      {
        const r = await get(baseUrl, "/error/async-reject");
        assert(ctx, "GET /error/async-reject → 5xx", () => r.status >= 500);
      }
      {
        const r = await postJson(baseUrl, "/error/missing-field", {});
        assert(ctx, "POST /error/missing-field with empty body → 422", () => r.status === 422);
      }
      {
        const r = await postJson(baseUrl, "/error/missing-field", { name: "Alice" });
        assert(ctx, "POST /error/missing-field with name → 200", () => r.status === 200);
        assert(ctx, "POST /error/missing-field with name → greeting", () =>
          (r.body as any)?.greeting === "Hello, Alice");
      }
    },
  },

];

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const suiteArg     = process.argv.indexOf("--suite");
const SUITE_FILTER = suiteArg !== -1 ? process.argv[suiteArg + 1] : undefined;

const activeSuites = SUITE_FILTER
  ? SUITES.filter((s) => s.name.includes(SUITE_FILTER))
  : SUITES;

async function main() {
  console.log("\n╔═══════════════════════════════════════════╗");
  console.log(  "║   Flux Runtime Integration Tests          ║");
  console.log(  "╚═══════════════════════════════════════════╝\n");

  // Build (or verify) the binary before doing anything else
  try {
    ensureBinary({ quiet: false });
  } catch (err) {
    console.error(`\nFailed to ensure flux-runtime binary:\n  ${(err as Error).message}\n`);
    process.exit(1);
  }

  let totalPassed = 0;
  let totalFailed = 0;

  interface SuiteReport {
    suite:   string;
    passed:  number;
    failed:  number;
    results: TestResult[];
  }
  const allReports: SuiteReport[] = [];

  for (const suite of activeSuites) {
    process.stdout.write(`  Running: ${suite.name} … `);
    const start = performance.now();

    const { passed, failed, results } = await runSuite(suite);

    const elapsed = (performance.now() - start).toFixed(0);
    const icon    = failed === 0 ? "✓" : "✗";
    console.log(`${icon}  ${passed}/${passed + failed} passed  (${elapsed}ms)`);

    if (failed > 0) {
      for (const r of results.filter((x) => !x.passed)) {
        console.log(`    ✗ ${r.name}: ${r.error ?? "failed"}`);
      }
    }

    totalPassed += passed;
    totalFailed += failed;
    allReports.push({ suite: suite.name, passed, failed, results });
  }

  // Write report
  const totalElapsed = allReports.reduce((s, r) => s + r.results.reduce((a, x) => a + x.duration, 0), 0);
  const report = buildReport("flux-integration", allReports.flatMap((r) => r.results), totalElapsed);
  writeReport("flux-integration", report);

  // Summary banner
  console.log("\n─────────────────────────────────────────────");
  const total = totalPassed + totalFailed;
  if (totalFailed === 0) {
    console.log(`  ✓  All ${total} integration checks passed.`);
  } else {
    console.log(`  ✗  ${totalFailed}/${total} checks FAILED.`);
  }
  console.log("─────────────────────────────────────────────\n");

  if (totalFailed > 0) process.exit(1);
}

main().catch((err) => {
  console.error("Unexpected error:", err);
  process.exit(1);
});
