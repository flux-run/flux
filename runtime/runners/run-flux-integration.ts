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
import { spawnSync, spawn } from "node:child_process";
import { generateKeyPairSync, sign as signWithNodeCrypto, type KeyObject } from "node:crypto";
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
const DB_THEN_REMOTE_DIR = resolve(EXAMPLES_DIR, "db_then_remote");
const DB_THEN_REMOTE_INIT_SQL = resolve(DB_THEN_REMOTE_DIR, "init.sql");
const DRIZZLE_DIR = resolve(EXAMPLES_DIR, "drizzle");
const IDEMPOTENCY_DIR = resolve(EXAMPLES_DIR, "idempotency");
const IDEMPOTENCY_INIT_SQL = resolve(IDEMPOTENCY_DIR, "init.sql");
const WEBHOOK_DEDUP_DIR = resolve(EXAMPLES_DIR, "webhook_dedup");
const WEBHOOK_DEDUP_INIT_SQL = resolve(WEBHOOK_DEDUP_DIR, "init.sql");
const JWKS_SERVER_ENTRY = resolve(HANDLERS_DIR, "jwks_server.js");

// Each suite gets its own port in the 3100-3199 range so suites can run
// sequentially without port conflicts when multiple are enabled.
let nextPort = 3100;
function allocatePort() { return nextPort++; }

let nextDatabasePort = 55432;
function allocateDatabasePort() { return nextDatabasePort++; }

let nextServerPort = 51051;
function allocateServerPort() { return nextServerPort++; }

let nextRemotePort = 39010;
function allocateRemotePort() { return nextRemotePort++; }

let nextRedisPort = 56379;
function allocateRedisPort() { return nextRedisPort++; }

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

interface RedisHandle {
  containerName: string;
  redisUrl: string;
  stop(): void;
}

interface RemoteHandle {
  baseUrl: string;
  stop(): Promise<void>;
}

const crudReplayState = {
  serverUrl: "",
  serviceToken: "",
};

const idempotencyState = {
  serverUrl: "",
  serviceToken: "",
};

const webhookDedupState = {
  serverUrl: "",
  serviceToken: "",
};

const dbThenRemoteResumeState = {
  serverUrl: "",
  serviceToken: "",
  remotePort: 0,
};

const jwksCacheState = {
  serverUrl: "",
  serviceToken: "",
  jwksPort: 0,
  jwksUrl: "",
};

const jwtAuthState = {
  serverUrl: "",
  serviceToken: "",
  jwksPort: 0,
  jwksUrl: "",
  issuer: "",
  audience: "flux-api",
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

async function waitForPostgres(
  containerName: string,
  username: string,
  databaseName: string,
  timeoutMs = 15_000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = spawnSync("docker", ["exec", containerName, "pg_isready", "-U", username, "-d", databaseName], {
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
    await waitForPostgres(containerName, config.username, config.databaseName);
    if (config.initSql) {
      const deadline = Date.now() + 15_000;
      let lastError: Error | null = null;
      while (Date.now() < deadline) {
        try {
          runCheckedCommand(
            "docker",
            ["exec", "-i", containerName, "psql", "-U", config.username, "-d", config.databaseName, "-v", "ON_ERROR_STOP=1"],
            { input: config.initSql },
          );
          lastError = null;
          break;
        } catch (error) {
          lastError = error as Error;
          await sleep(250);
        }
      }

      if (lastError) {
        throw lastError;
      }
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

async function waitForRedis(containerName: string, timeoutMs = 15_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = spawnSync("docker", ["exec", containerName, "redis-cli", "ping"], {
      cwd: WORKSPACE_ROOT,
      encoding: "utf8",
    });

    if (result.status === 0 && (result.stdout ?? "").includes("PONG")) {
      return;
    }

    await sleep(250);
  }

  throw new Error(`redis container ${containerName} did not become ready within ${timeoutMs}ms`);
}

async function startRedis(hostPort: number): Promise<RedisHandle> {
  const containerName = `flux-int-redis-${hostPort}`;
  const redisUrl = `redis://127.0.0.1:${hostPort}/0`;

  runCheckedCommand("docker", [
    "run",
    "--rm",
    "-d",
    "--name",
    containerName,
    "-p",
    `${hostPort}:6379`,
    "redis:7-alpine",
  ]);

  try {
    await waitForRedis(containerName);
  } catch (error) {
    runCheckedCommand("docker", ["rm", "-f", containerName]);
    throw error;
  }

  return {
    containerName,
    redisUrl,
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

async function startDbThenRemotePostgres(hostPort: number): Promise<PostgresHandle> {
  return startPostgres(hostPort, {
    databaseName: "db_then_remote",
    username: "admin",
    password: "password123",
    initSql: readFileSync(DB_THEN_REMOTE_INIT_SQL, "utf8"),
  });
}

async function startMockRemoteSystem(port: number): Promise<RemoteHandle> {
  const proc = spawn(process.execPath, [resolve(DB_THEN_REMOTE_DIR, "remote_system.js")], {
    cwd: WORKSPACE_ROOT,
    env: {
      ...process.env,
      PORT: String(port),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  proc.on("error", (error) => {
    throw new Error(`failed to start mock remote system: ${error.message}`);
  });

  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/__healthcheck__`);
      if (response.status === 404) {
        break;
      }
    } catch {
      await sleep(100);
      continue;
    }
  }

  if (Date.now() >= deadline) {
    proc.kill("SIGTERM");
    throw new Error(`mock remote system did not start within 15000ms on port ${port}`);
  }

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    async stop() {
      proc.kill("SIGTERM");
      await Promise.race([
        new Promise<void>((resolvePromise) => proc.once("exit", () => resolvePromise())),
        sleep(3000),
      ]);
      if (!proc.killed) {
        proc.kill("SIGKILL");
      }
    },
  };
}

async function startMockJwksServer(port: number, options: { jwksJson?: string } = {}): Promise<RemoteHandle> {
  const proc = spawn(process.execPath, [JWKS_SERVER_ENTRY], {
    cwd: WORKSPACE_ROOT,
    env: {
      ...process.env,
      PORT: String(port),
      ...(options.jwksJson ? { JWKS_JSON: options.jwksJson } : {}),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  proc.on("error", (error) => {
    throw new Error(`failed to start mock jwks server: ${error.message}`);
  });

  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/.well-known/jwks.json`);
      if (response.status === 200) {
        break;
      }
    } catch {
      await sleep(100);
      continue;
    }
  }

  if (Date.now() >= deadline) {
    proc.kill("SIGTERM");
    throw new Error(`mock jwks server did not start within 15000ms on port ${port}`);
  }

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    async stop() {
      proc.kill("SIGTERM");
      await Promise.race([
        new Promise<void>((resolvePromise) => proc.once("exit", () => resolvePromise())),
        sleep(3000),
      ]);
      if (!proc.killed) {
        proc.kill("SIGKILL");
      }
    },
  };
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

interface GeneratedRs256KeyPair {
  kid: string;
  privateKey: KeyObject;
  publicJwk: Record<string, unknown>;
}

function generateRs256KeyPair(kid = "test-key"): GeneratedRs256KeyPair {
  const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const publicJwk = publicKey.export({ format: "jwk" }) as Record<string, unknown>;

  return {
    kid,
    privateKey,
    publicJwk: {
      ...publicJwk,
      kid,
      alg: "RS256",
      use: "sig",
      key_ops: ["verify"],
    },
  };
}

function signJwtRs256(privateKey: KeyObject, kid: string, claims: Record<string, unknown>): string {
  const header = Buffer.from(JSON.stringify({ alg: "RS256", typ: "JWT", kid })).toString("base64url");
  const payload = Buffer.from(JSON.stringify(claims)).toString("base64url");
  const signingInput = `${header}.${payload}`;
  const signature = signWithNodeCrypto("RSA-SHA256", Buffer.from(signingInput), privateKey).toString("base64url");
  return `${signingInput}.${signature}`;
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

  // ── 4. Request isolation ─────────────────────────────────────────────────
  {
    name: "request-isolation",
    handler: "request-isolation.js",
    async run(baseUrl, ctx) {
      {
        const first = await get(baseUrl, "/counter");
        assert(ctx, "GET /counter first request → 200", () => first.status === 200);
        assert(ctx, "GET /counter first request → counter is 1", () => (first.body as any)?.counter === 1);
      }
      {
        const second = await get(baseUrl, "/counter");
        assert(ctx, "GET /counter second request → 200", () => second.status === 200);
        assert(ctx, "GET /counter second request → counter resets to 1", () => (second.body as any)?.counter === 1);
      }
      {
        const third = await get(baseUrl, "/object-id");
        assert(ctx, "GET /object-id → 200", () => third.status === 200);
        assert(ctx, "GET /object-id → request-local object starts at 1", () => (third.body as any)?.seen === 1);
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

  {
    name: "idempotency-redis",
    handler: "idempotency/main_flux.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const redisPort = allocateRedisPort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startPostgres(databasePort, {
        databaseName: "idempotency_demo",
        username: "admin",
        password: "password123",
        initSql: readFileSync(IDEMPOTENCY_INIT_SQL, "utf8"),
      });
      const redis = await startRedis(redisPort);
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        idempotencyState.serverUrl = server.url;
        idempotencyState.serviceToken = serviceToken;
        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            REDIS_URL: redis.redisUrl,
            FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLOWBASE_ALLOW_LOOPBACK_REDIS: "1",
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
                try {
                  redis.stop();
                } finally {
                  postgres.stop();
                  idempotencyState.serverUrl = "";
                  idempotencyState.serviceToken = "";
                }
              }
            }
          },
        };
      } catch (error) {
        idempotencyState.serverUrl = "";
        idempotencyState.serviceToken = "";
        await server.stop();
        redis.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const initialList = await get(baseUrl, "/orders");
      const initialOrders = (initialList.body as Record<string, unknown>)?.orders as unknown[] | undefined;
      assert(ctx, "GET /orders before create → 200", () => initialList.status === 200);
      assert(ctx, "GET /orders before create → empty", () => Array.isArray(initialOrders) && initialOrders.length === 0);

      const requestBody = {
        sku: "flux-shirt",
        quantity: 1,
      };
      const idempotencyKey = "order-123";

      const firstCreate = await fetch(`${baseUrl}/orders`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify(requestBody),
      });
      const firstBody = await firstCreate.json() as Record<string, unknown>;
      const executionId = firstCreate.headers.get("x-flux-execution-id") ?? "";

      assert(ctx, "POST /orders first request → 201", () => firstCreate.status === 201);
      assert(ctx, "POST /orders first request → execution id header", () => executionId.length > 0);
      assert(ctx, "POST /orders first request → created status header", () => firstCreate.headers.get("x-idempotency-status") === "created");
      assert(ctx, "POST /orders first request → order payload", () => {
        const order = firstBody.order as Record<string, unknown> | undefined;
        return order?.sku === "flux-shirt" && order?.quantity === 1;
      });

      const secondCreate = await fetch(`${baseUrl}/orders`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify(requestBody),
      });
      const secondBody = await secondCreate.json() as Record<string, unknown>;

      assert(ctx, "POST /orders second request → same status", () => secondCreate.status === 201);
      assert(ctx, "POST /orders second request → replayed status header", () => secondCreate.headers.get("x-idempotency-status") === "replayed");
      assert(ctx, "POST /orders second request → same JSON response", () => stableJson(secondBody) === stableJson(firstBody));

      const afterSecondList = await get(baseUrl, "/orders");
      const afterSecondOrders = (afterSecondList.body as Record<string, unknown>)?.orders as Array<Record<string, unknown>> | undefined;
      assert(ctx, "GET /orders after duplicate request → one row", () => Array.isArray(afterSecondOrders) && afterSecondOrders.length === 1);

      const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
        "replay",
        executionId,
        "--url",
        idempotencyState.serverUrl,
        "--token",
        idempotencyState.serviceToken,
        "--diff",
      ]));
      const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
        net_response?: { body?: string; status?: number };
      };
      const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;

      assert(ctx, "flux replay idempotent order → ok", () => replayStdout.includes("ok"));
      assert(ctx, "flux replay idempotent order → same JSON response", () => stableJson(replayBody) === stableJson(firstBody));
      assert(ctx, "flux replay idempotent order → 201 status preserved", () => replayEnvelope.net_response?.status === 201);
      assert(ctx, "flux replay idempotent order → Redis recorded", () => replayStdout.includes("REDIS") && replayStdout.includes("(recorded)"));
      assert(ctx, "flux replay idempotent order → Postgres recorded", () => replayStdout.includes("POSTGRES") && replayStdout.includes("(recorded)"));

      const afterReplayList = await get(baseUrl, "/orders");
      const afterReplayOrders = (afterReplayList.body as Record<string, unknown>)?.orders as Array<Record<string, unknown>> | undefined;
      assert(ctx, "GET /orders after replay → count unchanged", () => Array.isArray(afterReplayOrders) && afterReplayOrders.length === 1);
    },
  },

  {
    name: "webhook-dedup",
    handler: "webhook_dedup/main_flux.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const redisPort = allocateRedisPort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startPostgres(databasePort, {
        databaseName: "webhook_dedup",
        username: "admin",
        password: "password123",
        initSql: readFileSync(WEBHOOK_DEDUP_INIT_SQL, "utf8"),
      });
      const redis = await startRedis(redisPort);
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        webhookDedupState.serverUrl = server.url;
        webhookDedupState.serviceToken = serviceToken;
        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            REDIS_URL: redis.redisUrl,
            FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLOWBASE_ALLOW_LOOPBACK_REDIS: "1",
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
                try {
                  redis.stop();
                } finally {
                  postgres.stop();
                  webhookDedupState.serverUrl = "";
                  webhookDedupState.serviceToken = "";
                }
              }
            }
          },
        };
      } catch (error) {
        webhookDedupState.serverUrl = "";
        webhookDedupState.serviceToken = "";
        await server.stop();
        redis.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const initialList = await get(baseUrl, "/events");
      const initialEvents = (initialList.body as Record<string, unknown>)?.events as unknown[] | undefined;
      assert(ctx, "GET /events before webhook → 200", () => initialList.status === 200);
      assert(ctx, "GET /events before webhook → empty", () => Array.isArray(initialEvents) && initialEvents.length === 0);

      const eventId = "evt_123";
      const payload = {
        provider: "stripe",
        type: "invoice.paid",
      };

      const firstWebhook = await fetch(`${baseUrl}/webhook`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-event-id": eventId,
        },
        body: JSON.stringify(payload),
      });
      const firstBody = await firstWebhook.json() as Record<string, unknown>;
      const executionId = firstWebhook.headers.get("x-flux-execution-id") ?? "";

      assert(ctx, "POST /webhook first request → 202", () => firstWebhook.status === 202);
      assert(ctx, "POST /webhook first request → execution id header", () => executionId.length > 0);
      assert(ctx, "POST /webhook first request → processed header", () => firstWebhook.headers.get("x-webhook-status") === "processed");
      assert(ctx, "POST /webhook first request → processed body", () => firstBody.status === "processed" && firstBody.eventId === eventId);

      const secondWebhook = await fetch(`${baseUrl}/webhook`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-event-id": eventId,
        },
        body: JSON.stringify(payload),
      });
      const secondBody = await secondWebhook.json() as Record<string, unknown>;

      assert(ctx, "POST /webhook duplicate request → 200", () => secondWebhook.status === 200);
      assert(ctx, "POST /webhook duplicate request → duplicate header", () => secondWebhook.headers.get("x-webhook-status") === "duplicate");
      assert(ctx, "POST /webhook duplicate request → duplicate body", () => secondBody.status === "duplicate" && secondBody.eventId === eventId);

      const afterDuplicateList = await get(baseUrl, "/events");
      const afterDuplicateEvents = (afterDuplicateList.body as Record<string, unknown>)?.events as Array<Record<string, unknown>> | undefined;
      assert(ctx, "GET /events after duplicate webhook → one row", () => Array.isArray(afterDuplicateEvents) && afterDuplicateEvents.length === 1);

      const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
        "replay",
        executionId,
        "--url",
        webhookDedupState.serverUrl,
        "--token",
        webhookDedupState.serviceToken,
        "--diff",
      ]));
      const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
        net_response?: { body?: string; status?: number };
      };
      const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;

      assert(ctx, "flux replay webhook request → ok", () => replayStdout.includes("ok"));
      assert(ctx, "flux replay webhook request → same JSON response", () => stableJson(replayBody) === stableJson(firstBody));
      assert(ctx, "flux replay webhook request → 202 status preserved", () => replayEnvelope.net_response?.status === 202);
      assert(ctx, "flux replay webhook request → Redis recorded", () => replayStdout.includes("REDIS") && replayStdout.includes("(recorded)"));
      assert(ctx, "flux replay webhook request → Postgres recorded", () => replayStdout.includes("POSTGRES") && replayStdout.includes("(recorded)"));

      const afterReplayList = await get(baseUrl, "/events");
      const afterReplayEvents = (afterReplayList.body as Record<string, unknown>)?.events as Array<Record<string, unknown>> | undefined;
      assert(ctx, "GET /events after replay → count unchanged", () => Array.isArray(afterReplayEvents) && afterReplayEvents.length === 1);
    },
  },

  {
    name: "jwks-cache",
    handler: "jwks-cache.js",
    async start(entry, port) {
      const jwksPort = allocateRemotePort();
      const jwksUrl = `http://127.0.0.1:${jwksPort}/.well-known/jwks.json`;
      const databasePort = allocateDatabasePort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startPostgres(databasePort, {
        databaseName: "jwks_cache",
        username: "admin",
        password: "password123",
      });
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        jwksCacheState.serverUrl = server.url;
        jwksCacheState.serviceToken = serviceToken;
        jwksCacheState.jwksPort = jwksPort;
        jwksCacheState.jwksUrl = jwksUrl;
        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            FLOWBASE_ALLOW_LOOPBACK_FETCH: "1",
            JWKS_URL: jwksUrl,
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
                jwksCacheState.serverUrl = "";
                jwksCacheState.serviceToken = "";
                jwksCacheState.jwksPort = 0;
                jwksCacheState.jwksUrl = "";
              }
            }
          },
        };
      } catch (error) {
        jwksCacheState.serverUrl = "";
        jwksCacheState.serviceToken = "";
        jwksCacheState.jwksPort = 0;
        jwksCacheState.jwksUrl = "";
        await server.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const jwks = await startMockJwksServer(jwksCacheState.jwksPort);
      try {
        const liveRes = await fetch(`${baseUrl}/jwks`);
        const liveBody = await liveRes.json() as Record<string, unknown>;
        const liveExecutionId = liveRes.headers.get("x-flux-execution-id") ?? "";
        assert(ctx, "GET /jwks initial live request → 200", () => liveRes.status === 200);
        assert(ctx, "GET /jwks initial live request → execution id header", () => liveExecutionId.length > 0);
        assert(ctx, "GET /jwks initial live request → one key", () => liveBody.keys === 1 && liveBody.bypass === false);

        await jwks.stop();

        const cachedRes = await fetch(`${baseUrl}/jwks`);
        const cachedBody = await cachedRes.json() as Record<string, unknown>;
        assert(ctx, "GET /jwks after origin shutdown → 200", () => cachedRes.status === 200);
        assert(ctx, "GET /jwks after origin shutdown → cached body preserved", () => cachedBody.keys === 1 && cachedBody.bypass === false);

        const bypassRes = await fetch(`${baseUrl}/jwks-bypass`);
        const bypassBody = await bypassRes.json() as Record<string, unknown>;
        assert(ctx, "GET /jwks-bypass after origin shutdown → fails", () => bypassRes.status === 502);
        assert(ctx, "GET /jwks-bypass after origin shutdown → bypass flag true", () => bypassBody.bypass === true);

        const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "replay",
          liveExecutionId,
          "--url",
          jwksCacheState.serverUrl,
          "--token",
          jwksCacheState.serviceToken,
          "--diff",
        ]));
        const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
          net_response?: { body?: string; status?: number };
        };
        const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;

        assert(ctx, "flux replay jwks request → ok", () => replayStdout.includes("ok"));
        assert(ctx, "flux replay jwks request → 200 status preserved", () => replayEnvelope.net_response?.status === 200);
        assert(ctx, "flux replay jwks request → same JSON response", () => stableJson(replayBody) === stableJson(liveBody));
        assert(ctx, "flux replay jwks request → HTTP step recorded", () => replayStdout.includes("HTTP") && replayStdout.includes("(recorded)"));
      } finally {
        await jwks.stop().catch(() => undefined);
      }
    },
  },

  {
    name: "jwt-auth",
    handler: "jwt-auth.js",
    async start(entry, port) {
      const jwksPort = allocateRemotePort();
      const jwksUrl = `http://127.0.0.1:${jwksPort}/.well-known/jwks.json`;
      const issuer = `http://127.0.0.1:${jwksPort}/`;
      const databasePort = allocateDatabasePort();
      const serverPort = allocateServerPort();
      const serviceToken = "dev-service-token";
      const postgres = await startPostgres(databasePort, {
        databaseName: "jwt_auth",
        username: "admin",
        password: "password123",
      });
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        jwtAuthState.serverUrl = server.url;
        jwtAuthState.serviceToken = serviceToken;
        jwtAuthState.jwksPort = jwksPort;
        jwtAuthState.jwksUrl = jwksUrl;
        jwtAuthState.issuer = issuer;
        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            FLOWBASE_ALLOW_LOOPBACK_FETCH: "1",
            JWKS_URL: jwksUrl,
            JWT_ISSUER: issuer,
            JWT_AUDIENCE: jwtAuthState.audience,
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
                jwtAuthState.serverUrl = "";
                jwtAuthState.serviceToken = "";
                jwtAuthState.jwksPort = 0;
                jwtAuthState.jwksUrl = "";
                jwtAuthState.issuer = "";
              }
            }
          },
        };
      } catch (error) {
        jwtAuthState.serverUrl = "";
        jwtAuthState.serviceToken = "";
        jwtAuthState.jwksPort = 0;
        jwtAuthState.jwksUrl = "";
        jwtAuthState.issuer = "";
        await server.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const keyPair = generateRs256KeyPair("jwt-auth-key");
      const issuedAt = Math.floor(Date.now() / 1000);
      const validToken = signJwtRs256(keyPair.privateKey, keyPair.kid, {
        sub: "user-123",
        scope: "read:messages",
        iss: jwtAuthState.issuer,
        aud: jwtAuthState.audience,
        iat: issuedAt,
        exp: issuedAt + 3600,
      });
      const wrongAudienceToken = signJwtRs256(keyPair.privateKey, keyPair.kid, {
        sub: "user-123",
        scope: "read:messages",
        iss: jwtAuthState.issuer,
        aud: "wrong-audience",
        iat: issuedAt,
        exp: issuedAt + 3600,
      });
      const wrongKeyToken = signJwtRs256(generateRs256KeyPair("wrong-key").privateKey, "jwt-auth-key", {
        sub: "user-123",
        scope: "read:messages",
        iss: jwtAuthState.issuer,
        aud: jwtAuthState.audience,
        iat: issuedAt,
        exp: issuedAt + 3600,
      });
      const jwks = await startMockJwksServer(jwtAuthState.jwksPort, {
        jwksJson: JSON.stringify({ keys: [keyPair.publicJwk] }),
      });

      try {
        const publicRes = await fetch(`${baseUrl}/public`);
        const publicBody = await publicRes.json() as Record<string, unknown>;
        assert(ctx, "GET /public without auth → 200", () => publicRes.status === 200);
        assert(ctx, "GET /public without auth → unprotected route", () => publicBody.protected === false);

        const missingAuthRes = await fetch(`${baseUrl}/protected`);
        const missingAuthBody = await missingAuthRes.json() as Record<string, unknown>;
        assert(ctx, "GET /protected without bearer token → 401", () => missingAuthRes.status === 401);
        assert(ctx, "GET /protected without bearer token → error message", () => missingAuthBody.error === "missing bearer token");

        const liveRes = await fetch(`${baseUrl}/protected`, {
          headers: { authorization: `Bearer ${validToken}` },
        });
        const liveBody = await liveRes.json() as Record<string, unknown>;
        const liveExecutionId = liveRes.headers.get("x-flux-execution-id") ?? "";
        assert(ctx, "GET /protected with valid RS256 JWT → 200", () => liveRes.status === 200);
        assert(ctx, "GET /protected with valid RS256 JWT → execution id header", () => liveExecutionId.length > 0);
        assert(ctx, "GET /protected with valid RS256 JWT → payload surfaced", () => liveBody.sub === "user-123" && liveBody.scope === "read:messages");

        const wrongAudienceRes = await fetch(`${baseUrl}/protected`, {
          headers: { authorization: `Bearer ${wrongAudienceToken}` },
        });
        const wrongAudienceBody = await wrongAudienceRes.json() as Record<string, unknown>;
        assert(ctx, "GET /protected with wrong audience → 401", () => wrongAudienceRes.status === 401);
        assert(ctx, "GET /protected with wrong audience → claim validation error", () => wrongAudienceBody.error === "audience mismatch");

        await jwks.stop();

        const cachedRes = await fetch(`${baseUrl}/protected`, {
          headers: { authorization: `Bearer ${validToken}` },
        });
        const cachedBody = await cachedRes.json() as Record<string, unknown>;
        assert(ctx, "GET /protected after JWKS origin shutdown → 200", () => cachedRes.status === 200);
        assert(ctx, "GET /protected after JWKS origin shutdown → cached key still verifies", () => cachedBody.sub === "user-123" && cachedBody.protected === true);

        const wrongKeyRes = await fetch(`${baseUrl}/protected`, {
          headers: { authorization: `Bearer ${wrongKeyToken}` },
        });
        const wrongKeyBody = await wrongKeyRes.json() as Record<string, unknown>;
        assert(ctx, "GET /protected with wrong signature → 401", () => wrongKeyRes.status === 401);
        assert(ctx, "GET /protected with wrong signature → signature failure", () => wrongKeyBody.error === "invalid signature");

        const bypassRes = await fetch(`${baseUrl}/protected-bypass`, {
          headers: { authorization: `Bearer ${validToken}` },
        });
        const bypassBody = await bypassRes.json() as Record<string, unknown>;
        assert(ctx, "GET /protected-bypass after JWKS origin shutdown → 503", () => bypassRes.status === 503);
        assert(ctx, "GET /protected-bypass after JWKS origin shutdown → bypass flag true", () => bypassBody.bypass === true);

        const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "replay",
          liveExecutionId,
          "--url",
          jwtAuthState.serverUrl,
          "--token",
          jwtAuthState.serviceToken,
          "--diff",
        ]));
        const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
          net_response?: { body?: string; status?: number };
        };
        const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;
        assert(ctx, "flux replay protected JWT request → ok", () => replayStdout.includes("ok"));
        assert(ctx, "flux replay protected JWT request → 200 status preserved", () => replayEnvelope.net_response?.status === 200);
        assert(ctx, "flux replay protected JWT request → same JSON response", () => stableJson(replayBody) === stableJson(liveBody));
        assert(ctx, "flux replay protected JWT request → HTTP step recorded", () => replayStdout.includes("HTTP") && replayStdout.includes("(recorded)"));
      } finally {
        await jwks.stop().catch(() => undefined);
      }
    },
  },

  {
    name: "db-then-remote-resume",
    handler: "db_then_remote/main_flux.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const serverPort = allocateServerPort();
      const remotePort = allocateRemotePort();
      const serviceToken = "dev-service-token";
      const postgres = await startDbThenRemotePostgres(databasePort);
      const server = await startServer(serverPort, {
        databaseUrl: postgres.databaseUrl,
        serviceToken,
      });

      try {
        dbThenRemoteResumeState.serverUrl = server.url;
        dbThenRemoteResumeState.serviceToken = serviceToken;
        dbThenRemoteResumeState.remotePort = remotePort;

        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            FLOWBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLOWBASE_ALLOW_LOOPBACK_FETCH: "1",
            REMOTE_BASE_URL: `http://127.0.0.1:${remotePort}`,
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
                dbThenRemoteResumeState.serverUrl = "";
                dbThenRemoteResumeState.serviceToken = "";
                dbThenRemoteResumeState.remotePort = 0;
              }
            }
          },
        };
      } catch (error) {
        dbThenRemoteResumeState.serverUrl = "";
        dbThenRemoteResumeState.serviceToken = "";
        dbThenRemoteResumeState.remotePort = 0;
        await server.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const initialList = await get(baseUrl, "/dispatches");
      const initialRows = initialList.body as any[];
      assert(ctx, "GET /dispatches before failure → 200", () => initialList.status === 200);
      assert(ctx, "GET /dispatches before failure → empty", () => Array.isArray(initialRows) && initialRows.length === 0);

      const failedRes = await fetch(`${baseUrl}/dispatches`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          orderId: "order-1",
          message: "first-dispatch",
        }),
      });
      const failedExecutionId = failedRes.headers.get("x-flux-execution-id") ?? "";

      assert(ctx, "POST /dispatches with remote offline → 500", () => failedRes.status === 500);
      assert(ctx, "POST /dispatches with remote offline → execution id header", () => failedExecutionId.length > 0);

      const listAfterFailure = await get(baseUrl, "/dispatches");
      const pendingRows = listAfterFailure.body as Array<Record<string, unknown>>;
      const pendingRow = Array.isArray(pendingRows) ? pendingRows[0] : undefined;
      assert(ctx, "GET /dispatches after failure → one pending row", () => Array.isArray(pendingRows) && pendingRows.length === 1);
      assert(ctx, "GET /dispatches after failure → row is pending", () => pendingRow?.status === "pending");
      assert(ctx, "GET /dispatches after failure → remoteStatus absent", () => pendingRow?.remoteStatus == null);

      const remote = await startMockRemoteSystem(dbThenRemoteResumeState.remotePort);
      try {
        const resumeStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "resume",
          failedExecutionId,
          "--url",
          dbThenRemoteResumeState.serverUrl,
          "--token",
          dbThenRemoteResumeState.serviceToken,
        ]));

        const resumeEnvelope = JSON.parse(extractReplayOutput(resumeStdout)) as {
          net_response?: { body?: string; status?: number };
        };
        const resumeBody = JSON.parse(resumeEnvelope.net_response?.body ?? "null") as {
          dispatch?: { id?: number; status?: string; remoteStatus?: number; orderId?: string };
          remote?: { status?: number };
        };

        assert(ctx, "flux resume → starts after recorded checkpoint", () => resumeStdout.includes("from checkpoint 1"));
        assert(ctx, "flux resume → first Postgres step recorded", () => resumeStdout.includes("[0] POSTGRES") && resumeStdout.includes("(recorded)"));
        assert(ctx, "flux resume → remote HTTP step live", () => resumeStdout.includes("[1] HTTP") && resumeStdout.includes("(live)"));
        assert(ctx, "flux resume → completion Postgres step live", () => resumeStdout.includes("[2] POSTGRES") && resumeStdout.includes("(live)"));
        assert(ctx, "flux resume → returns 201", () => resumeEnvelope.net_response?.status === 201);
        assert(ctx, "flux resume → response marks dispatch delivered", () => resumeBody.dispatch?.status === "delivered" && resumeBody.remote?.status === 200);

        const listAfterResume = await get(baseUrl, "/dispatches");
        const resumedRows = listAfterResume.body as Array<Record<string, unknown>>;
        const resumedRow = Array.isArray(resumedRows) ? resumedRows[0] : undefined;

        assert(ctx, "GET /dispatches after resume → still one row", () => Array.isArray(resumedRows) && resumedRows.length === 1);
        assert(ctx, "GET /dispatches after resume → same row id reused", () => resumedRow?.id === pendingRow?.id && resumedRow?.id === resumeBody.dispatch?.id);
        assert(ctx, "GET /dispatches after resume → row delivered", () => resumedRow?.status === "delivered");
        assert(ctx, "GET /dispatches after resume → remoteStatus recorded", () => resumedRow?.remoteStatus === 200);
        assert(ctx, "GET /dispatches after resume → original order retained", () => resumedRow?.orderId === "order-1");
      } finally {
        await remote.stop();
      }
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
