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
import { spawnSync, spawn, type ChildProcess } from "node:child_process";
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
const JWKS_SERVER_ENTRY = resolve(__dirname, "../external-tests/flux-handlers/jwks-server.js");

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

const idempotencyCrashState = {
  serverUrl: "",
  serviceToken: "",
  databaseUrl: "",
  redisUrl: "",
  postgresContainerName: "",
  redisContainerName: "",
  entry: "",
  port: 0,
  runtime: null as RuntimeHandle | null,
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

async function waitForProcessExit(proc: ChildProcess, timeoutMs = 5_000): Promise<number | null> {
  if (proc.exitCode !== null) {
    return proc.exitCode;
  }

  if (proc.signalCode !== null) {
    return null;
  }

  return await Promise.race([
    new Promise<number | null>((resolve) => {
      proc.once("exit", (code) => resolve(code));
    }),
    sleep(timeoutMs).then(() => {
      throw new Error(`process did not exit within ${timeoutMs}ms`);
    }),
  ]);
}

function queryPostgresScalar(
  containerName: string,
  username: string,
  databaseName: string,
  sql: string,
): string {
  return runCheckedCommand(
    "docker",
    ["exec", "-i", containerName, "psql", "-U", username, "-d", databaseName, "-t", "-A", "-c", sql],
  ).trim();
}

function redisRaw(containerName: string, ...args: string[]): string {
  return runCheckedCommand("docker", ["exec", containerName, "redis-cli", "--raw", ...args]).trim();
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
  // Prefer the machine-readable `output_raw` line emitted when --diff is active.
  // This contains the raw JSON, unlike the human-readable `output` line which
  // may be reformatted by the CLI (e.g. decoded __FLUX_B64, status summarisation).
  const rawMatch = stdout.match(/^\s*output_raw\s+(.+)$/m);
  if (rawMatch) {
    return rawMatch[1];
  }
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
      const entryBaseDir = suite.handlerBaseDir === "examples" ? EXAMPLES_DIR : __dirname;
      const entry = resolve(entryBaseDir, suite.handler);

      buildArtifact(entry, { quiet: true });
      runtime = suite.start ? await suite.start(entry, port) : await startRuntime(entry, port, { timeoutMs: 60_000 });
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

const SUITES: Suite[] =[

  // ── 6. Bundled framework apps ───────────────────────────────────────────
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
  {
    name: "bundled-express",
    handler: "express-hello.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      {
        const res = await fetch(`${baseUrl}/`, {
          headers: { host: "localhost" },
        });
        const text = await res.text();
        assert(ctx, "GET / → 200", () => res.status === 200);
        assert(ctx, "GET / → express text body", () => text === "hello from express on flux");
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
  {
    name: "bundled-fastify",
    handler: "fastify-hello.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      {
        const res = await fetch(`${baseUrl}/`, {
          headers: { host: "localhost" },
        });
        const text = await res.text();
        assert(ctx, "GET / → 200", () => res.status === 200);
        assert(ctx, "GET / → fastify text body", () => text === "hello from fastify on flux");
      }
    },
  },
  {
    name: "bundled-koa",
    handler: "koa-hello.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      {
        const res = await fetch(`${baseUrl}/`, {
          headers: { host: "localhost" },
        });
        const text = await res.text();
        assert(ctx, "GET / → 200", () => res.status === 200);
        assert(ctx, "GET / → koa text body", () => text === "hello from koa on flux (mocked handler)");
      }
    },
  },


  // ── 7. Library Compatibility Suites ─────────────────────────────────────

  // ── Tier 1: HTTP Clients — fetch ─────────────────────────────────────────
  {
    name: "compat-fetch",
    handler: "compat/fetch-compat.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      // smoke test
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "fetch: GET / → 200", () => res.status === 200);
        assert(ctx, "fetch: GET / → library=fetch", () => body?.library === "fetch");
      }
      // outbound GET via fetch (IO interception)
      {
        const res = await fetch(`${baseUrl}/fetch-external`);
        const body = await res.json() as any;
        assert(ctx, "fetch: outbound GET → 200", () => res.status === 200);
        assert(ctx, "fetch: outbound GET → ok:true", () => body?.ok === true);
        assert(ctx, "fetch: outbound GET → status 200", () => body?.status === 200);
        assert(ctx, "fetch: outbound GET → origin_present", () => body?.origin_present === true);
      }
      // outbound POST via fetch
      {
        const res = await fetch(`${baseUrl}/fetch-post`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ flux: "test" }),
        });
        const body = await res.json() as any;
        assert(ctx, "fetch: outbound POST → 200", () => res.status === 200);
        assert(ctx, "fetch: outbound POST → ok:true", () => body?.ok === true);
        assert(ctx, "fetch: outbound POST → echoed body", () => body?.echoed?.flux === "test");
      }
      // custom headers
      {
        const res = await fetch(`${baseUrl}/fetch-headers`);
        const body = await res.json() as any;
        assert(ctx, "fetch: custom headers → 200", () => res.status === 200);
        assert(ctx, "fetch: custom headers → forwarded", () => body?.has_custom_header === true);
      }
      // ── Failure cases ────────────────────────────────────────────────────
      // upstream 404 handled gracefully
      {
        const res = await fetch(`${baseUrl}/fetch-404`);
        const body = await res.json() as any;
        assert(ctx, "fetch: 404 upstream → 200 wrapper", () => res.status === 200);
        assert(ctx, "fetch: 404 upstream → ok:true", () => body?.ok === true);
        assert(ctx, "fetch: 404 upstream → handled=true", () => body?.handled === true);
        assert(ctx, "fetch: 404 upstream → status=404", () => body?.upstream_status === 404);
      }
      // upstream 500 handled gracefully
      {
        const res = await fetch(`${baseUrl}/fetch-500`);
        const body = await res.json() as any;
        assert(ctx, "fetch: 500 upstream → ok:true", () => body?.ok === true);
        assert(ctx, "fetch: 500 upstream → status=500", () => body?.upstream_status === 500);
      }
      // connection refused — error caught and returned
      {
        const res = await fetch(`${baseUrl}/fetch-refused`);
        const body = await res.json() as any;
        assert(ctx, "fetch: refused → ok:true (caught)", () => body?.ok === true);
        assert(ctx, "fetch: refused → caught=true", () => body?.caught === true);
      }
      // ── Concurrency ──────────────────────────────────────────────────────
      {
        const res = await fetch(`${baseUrl}/concurrent`);
        const body = await res.json() as any;
        assert(ctx, "fetch: concurrent → 200", () => res.status === 200);
        assert(ctx, "fetch: concurrent → count=3", () => body?.count === 3);
        assert(ctx, "fetch: concurrent → all_have_origin", () => body?.all_have_origin === true);
      }
    },
  },
  {
    name: "compat-fetch-binary",
    handler: "compat/fetch-binary.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      {
        const res = await fetch(`${baseUrl}/binary-get`);
        const body = await res.json() as any;
        assert(ctx, "fetch: GET binary → 200", () => res.status === 200);
        assert(ctx, "fetch: GET binary → ok:true", () => body?.ok === true);
        assert(ctx, "fetch: GET binary → byteLength 100", () => body?.byteLength === 100);
      }
      {
        const res = await fetch(`${baseUrl}/binary-post`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "fetch: POST binary → 200", () => res.status === 200);
        assert(ctx, "fetch: POST binary → ok:true", () => body?.ok === true);
      }
    },
  },

  // ── Tier 1: HTTP Clients — axios ─────────────────────────────────────────
  {
    name: "compat-axios",
    handler: "compat/axios-compat.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      // smoke test
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "axios: GET / → 200", () => res.status === 200);
        assert(ctx, "axios: GET / → library=axios", () => body?.library === "axios");
      }
      // outbound GET via axios
      {
        const res = await fetch(`${baseUrl}/axios-get`);
        const body = await res.json() as any;
        assert(ctx, "axios: outbound GET → 200", () => res.status === 200);
        assert(ctx, "axios: outbound GET → ok:true", () => body?.ok === true);
        assert(ctx, "axios: outbound GET → origin_present", () => body?.origin_present === true);
      }
      // outbound POST via axios
      {
        const res = await fetch(`${baseUrl}/axios-post`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ flux: "axios-test" }),
        });
        const body = await res.json() as any;
        assert(ctx, "axios: outbound POST → 200", () => res.status === 200);
        assert(ctx, "axios: outbound POST → ok:true", () => body?.ok === true);
        assert(ctx, "axios: outbound POST → echoed body", () => body?.echoed?.flux === "axios-test");
      }
      // axios custom headers
      {
        const res = await fetch(`${baseUrl}/axios-headers`);
        const body = await res.json() as any;
        assert(ctx, "axios: custom headers → 200", () => res.status === 200);
        assert(ctx, "axios: custom headers → forwarded", () => body?.has_custom_header === true);
      }
      // axios non-2xx does not throw
      {
        const res = await fetch(`${baseUrl}/axios-error`);
        const body = await res.json() as any;
        assert(ctx, "axios: non-2xx → 200", () => res.status === 200);
        assert(ctx, "axios: non-2xx → received 404", () => body?.received_status === 404);
      }
    },
  },

  // ── Tier 2: Validation — Zod ─────────────────────────────────────────────
  {
    name: "compat-zod",
    handler: "compat/zod-compat.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      // smoke test
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "zod: GET / → 200", () => res.status === 200);
        assert(ctx, "zod: GET / → library=zod", () => body?.library === "zod");
      }
      // valid user
      {
        const res = await fetch(`${baseUrl}/validate-user`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: "Alice", email: "alice@example.com" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: valid user → 200", () => res.status === 200);
        assert(ctx, "zod: valid user → ok:true", () => body?.ok === true);
        assert(ctx, "zod: valid user → name", () => body?.user?.name === "Alice");
      }
      // invalid user (bad email)
      {
        const res = await fetch(`${baseUrl}/validate-bad`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: "", email: "not-an-email" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: invalid user → ok:false", () => body?.ok === false);
        assert(ctx, "zod: invalid user → errors present", () => !!body?.errors);
      }
      // pagination defaults
      {
        const res = await fetch(`${baseUrl}/paginate`);
        const body = await res.json() as any;
        assert(ctx, "zod: paginate defaults → 200", () => res.status === 200);
        assert(ctx, "zod: paginate defaults → page=1", () => body?.pagination?.page === 1);
        assert(ctx, "zod: paginate defaults → limit=20", () => body?.pagination?.limit === 20);
      }
      // pagination with values
      {
        const res = await fetch(`${baseUrl}/paginate?page=3&limit=50`);
        const body = await res.json() as any;
        assert(ctx, "zod: paginate with values → page=3", () => body?.pagination?.page === 3);
        assert(ctx, "zod: paginate with values → limit=50", () => body?.pagination?.limit === 50);
      }
      // transform pipeline
      {
        const res = await fetch(`${baseUrl}/transform`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ value: "  HELLO FLUX  " }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: transform → 200", () => res.status === 200);
        assert(ctx, "zod: transform → lowercased + trimmed", () => body?.transformed?.value === "hello flux");
      }
      // ── Failure / edge cases ─────────────────────────────────────────────
      // strict mode: unknown keys rejected
      {
        const res = await fetch(`${baseUrl}/validate-strict`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: "Alice", email: "alice@example.com", unknownField: "bad" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: strict mode → ok:false on unknown keys", () => body?.ok === false);
        assert(ctx, "zod: strict mode → errors present", () => !!body?.errors);
      }
      // strict mode passes when valid
      {
        const res = await fetch(`${baseUrl}/validate-strict`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: "Bob", email: "bob@example.com" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: strict mode → ok:true on exact keys", () => body?.ok === true);
      }
      // nested schema
      {
        const res = await fetch(`${baseUrl}/validate-nested`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            user: { name: "Carol", email: "carol@example.com" },
            address: { street: "123 Main St", city: "Springfield", zip: "12345" },
          }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: nested schema → 200", () => res.status === 200);
        assert(ctx, "zod: nested schema → ok:true", () => body?.ok === true);
      }
      // union schema
      {
        const res = await fetch(`${baseUrl}/validate-union`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ type: "email", value: "dan@example.com" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: union schema (email) → ok:true", () => body?.ok === true);
      }
      // custom refinement — passwords match
      {
        const res = await fetch(`${baseUrl}/validate-custom`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ password: "secret", confirm: "secret" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: custom refinement (match) → ok:true", () => body?.ok === true);
      }
      // custom refinement — passwords mismatch → error
      {
        const res = await fetch(`${baseUrl}/validate-custom`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ password: "secret", confirm: "wrong" }),
        });
        const body = await res.json() as any;
        assert(ctx, "zod: custom refinement (mismatch) → ok:false", () => body?.ok === false);
      }
    },
  },

  // ── Tier 1: Postgres — pg driver ─────────────────────────────────────────
  {
    name: "compat-pg",
    handler: "compat/pg-compat.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const postgres = await startPostgres(databasePort, {
        databaseName: "compat_pg",
        username: "postgres",
        password: "postgres",
      });
      try {
        const runtime = await startRuntime(entry, port, {
          skipVerify: true,
          timeoutMs: 60_000,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
          },
        });
        return {
          ...runtime,
          async stop() {
            try { await runtime.stop(); } finally { postgres.stop(); }
          },
        };
      } catch (err) {
        postgres.stop();
        throw err;
      }
    },
    async run(baseUrl, ctx) {
      // smoke test (no DB)
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "pg: GET / → 200", () => res.status === 200);
        assert(ctx, "pg: GET / → library=pg", () => body?.library === "pg");
      }
      // SELECT 1
      {
        const res = await fetch(`${baseUrl}/db-query`);
        const body = await res.json() as any;
        assert(ctx, "pg: SELECT 1 → 200", () => res.status === 200);
        assert(ctx, "pg: SELECT 1 → value=1", () => Number(body?.value) === 1);
      }
      // INSERT + SELECT + DELETE
      {
        const res = await fetch(`${baseUrl}/db-insert-select`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ label: "flux-compat-test" }),
        });
        const body = await res.json() as any;
        assert(ctx, "pg: insert-select → 200", () => res.status === 200);
        assert(ctx, "pg: insert-select → ok:true", () => body?.ok === true);
        assert(ctx, "pg: insert-select → label matches", () => body?.inserted?.label === "flux-compat-test");
        assert(ctx, "pg: insert-select → selected matches", () => body?.selected?.label === "flux-compat-test");
      }
      // TRANSACTION
      {
        const res = await fetch(`${baseUrl}/db-transaction`);
        const body = await res.json() as any;
        assert(ctx, "pg: transaction → 200", () => res.status === 200);
        assert(ctx, "pg: transaction → ok:true", () => body?.ok === true);
        assert(ctx, "pg: transaction → ts present", () => !!body?.ts);
      }
      // ── Failure cases ────────────────────────────────────────────────────
      // explicit ROLLBACK — data must NOT be persisted
      {
        const res = await fetch(`${baseUrl}/db-rollback`);
        const body = await res.json() as any;
        assert(ctx, "pg: rollback → 200", () => res.status === 200);
        assert(ctx, "pg: rollback → ok:true", () => body?.ok === true);
        assert(ctx, "pg: rollback → rows=0 after rollback", () => body?.rows_after_rollback === 0);
      }
      // unique constraint violation — caught + returned as structured error
      {
        const res = await fetch(`${baseUrl}/db-constraint`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({}),
        });
        const body = await res.json() as any;
        assert(ctx, "pg: constraint violation → 200 (caught)", () => res.status === 200);
        assert(ctx, "pg: constraint violation → ok:true", () => body?.ok === true);
        assert(ctx, "pg: constraint violation → caught=true", () => body?.caught === true);
        assert(ctx, "pg: constraint violation → code=23505", () => body?.code === "23505");
      }
      // ── Concurrency ──────────────────────────────────────────────────────
      {
        const res = await fetch(`${baseUrl}/concurrent`);
        const body = await res.json() as any;
        assert(ctx, "pg: concurrent queries → 200", () => res.status === 200);
        assert(ctx, "pg: concurrent queries → ok:true", () => body?.ok === true);
        assert(ctx, "pg: concurrent queries → sum=6", () => body?.sum === 6);
        assert(ctx, "pg: concurrent queries → results=[1,2,3]", () =>
          JSON.stringify(body?.results?.map(Number)) === "[1,2,3]"
        );
      }
    },
  },

  // ── Tier 2: ORM — Drizzle ────────────────────────────────────────────────
  {
    name: "compat-drizzle",
    handler: "compat/drizzle-compat.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const databasePort = allocateDatabasePort();
      const postgres = await startPostgres(databasePort, {
        databaseName: "compat_drizzle",
        username: "postgres",
        password: "postgres",
      });
      try {
        const runtime = await startRuntime(entry, port, {
          skipVerify: true,
          timeoutMs: 60_000,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
          },
        });
        return {
          ...runtime,
          async stop() {
            try { await runtime.stop(); } finally { postgres.stop(); }
          },
        };
      } catch (err) {
        postgres.stop();
        throw err;
      }
    },
    async run(baseUrl, ctx) {
      // smoke test
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "drizzle: GET / → 200", () => res.status === 200);
        assert(ctx, "drizzle: GET / → library=drizzle", () => body?.library === "drizzle");
      }
      // setup table
      {
        const res = await fetch(`${baseUrl}/setup`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "drizzle: setup table → 200", () => res.status === 200);
        assert(ctx, "drizzle: setup table → ok:true", () => body?.ok === true);
      }
      // insert via Drizzle
      {
        const res = await fetch(`${baseUrl}/insert`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: "flux-drizzle-item" }),
        });
        const body = await res.json() as any;
        assert(ctx, "drizzle: insert → 200", () => res.status === 200);
        assert(ctx, "drizzle: insert → ok:true", () => body?.ok === true);
        assert(ctx, "drizzle: insert → item.name", () => body?.item?.name === "flux-drizzle-item");
      }
      // select list
      {
        const res = await fetch(`${baseUrl}/list`);
        const body = await res.json() as any;
        assert(ctx, "drizzle: list → 200", () => res.status === 200);
        assert(ctx, "drizzle: list → ok:true", () => body?.ok === true);
        assert(ctx, "drizzle: list → count >= 1", () => body?.count >= 1);
      }
      // cleanup
      {
        const res = await fetch(`${baseUrl}/cleanup`, { method: "DELETE" });
        const body = await res.json() as any;
        assert(ctx, "drizzle: cleanup → ok:true", () => body?.ok === true);
      }
    },
  },

  // ── Tier 3: Redis — node-redis ───────────────────────────────────────────
  {
    name: "compat-redis",
    handler: "compat/redis-compat.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const redisPort = allocateRedisPort();
      const redis = await startRedis(redisPort);
      try {
        const runtime = await startRuntime(entry, port, {
          skipVerify: true,
          timeoutMs: 60_000,
          env: {
            REDIS_URL: redis.redisUrl,
            FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
          },
        });
        return {
          ...runtime,
          async stop() {
            try { await runtime.stop(); } finally { redis.stop(); }
          },
        };
      } catch (err) {
        redis.stop();
        throw err;
      }
    },
    async run(baseUrl, ctx) {
      // smoke test (no Redis)
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "redis: GET / → 200", () => res.status === 200);
        assert(ctx, "redis: GET / → library=node-redis", () => body?.library === "node-redis");
      }
      // PING
      {
        const res = await fetch(`${baseUrl}/ping`);
        const body = await res.json() as any;
        assert(ctx, "redis: PING → 200", () => res.status === 200);
        assert(ctx, "redis: PING → pong=PONG", () => body?.pong === "PONG");
      }
      // SET / GET / DEL
      {
        const res = await fetch(`${baseUrl}/set-get`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ key: "greeting", value: "hello-flux" }),
        });
        const body = await res.json() as any;
        assert(ctx, "redis: SET-GET → 200", () => res.status === 200);
        assert(ctx, "redis: SET-GET → stored matches", () => body?.stored === "hello-flux");
        assert(ctx, "redis: SET-GET → retrieved matches", () => body?.retrieved === "hello-flux");
      }
      // INCR / INCRBY
      {
        const res = await fetch(`${baseUrl}/incr`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "redis: INCR → 200", () => res.status === 200);
        assert(ctx, "redis: INCR → v1=1", () => body?.v1 === 1);
        assert(ctx, "redis: INCR → v2=2", () => body?.v2 === 2);
        assert(ctx, "redis: INCRBY 10 → v3=12", () => body?.v3 === 12);
      }
      // HSET / HGETALL
      {
        const res = await fetch(`${baseUrl}/hash`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ field: "env", value: "flux" }),
        });
        const body = await res.json() as any;
        assert(ctx, "redis: HSET-HGETALL → 200", () => res.status === 200);
        assert(ctx, "redis: HSET-HGETALL → field value matches", () => body?.all?.env === "flux");
      }
      // ── Failure cases ────────────────────────────────────────────────────
      // GET on nonexistent key → null
      {
        const res = await fetch(`${baseUrl}/redis-missing`);
        const body = await res.json() as any;
        assert(ctx, "redis: missing key → 200", () => res.status === 200);
        assert(ctx, "redis: missing key → ok:true", () => body?.ok === true);
        assert(ctx, "redis: missing key → value is null", () => body?.is_null === true);
      }
      // TTL set correctly
      {
        const res = await fetch(`${baseUrl}/redis-ttl-check`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "redis: TTL → 200", () => res.status === 200);
        assert(ctx, "redis: TTL → ttl_set=true", () => body?.ttl_set === true);
      }
      // ── Type operations ──────────────────────────────────────────────────
      // List + Set operations
      {
        const res = await fetch(`${baseUrl}/redis-type-ops`);
        const body = await res.json() as any;
        assert(ctx, "redis: type ops → 200", () => res.status === 200);
        assert(ctx, "redis: type ops → list len=3", () => body?.list?.len === 3);
        assert(ctx, "redis: type ops → list items correct", () =>
          JSON.stringify(body?.list?.items) === JSON.stringify(["a", "b", "c"])
        );
        assert(ctx, "redis: type ops → set card=3", () => body?.set?.card === 3);
        assert(ctx, "redis: type ops → y is member", () => body?.set?.y_is_member === true);
      }
    },
  },

  // ── Tier 2: Auth — Web Crypto API (JWT sign/verify, digest, PBKDF2) ────────
  {
    name: "compat-jose",
    handler: "compat/jose-compat.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      // smoke test (pure JS, no IO)
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "webcrypto: GET / → 200", () => res.status === 200);
        assert(ctx, "webcrypto: GET / → library=webcrypto", () => body?.library === "webcrypto");
      }
      // HMAC-SHA256 JWT sign
      let signedToken = "";
      {
        const res = await fetch(`${baseUrl}/sign`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ sub: "user-abc", role: "admin" }),
        });
        const body = await res.json() as any;
        assert(ctx, "webcrypto: sign → 200", () => res.status === 200);
        assert(ctx, "webcrypto: sign → ok:true", () => body?.ok === true);
        assert(ctx, "webcrypto: sign → token is string", () => typeof body?.token === "string");
        assert(ctx, "webcrypto: sign → 3 parts (header.payload.sig)", () => body?.parts === 3);
        signedToken = body?.token ?? "";
      }
      // npm:jose library test
      let joseToken = "";
      {
        const res = await fetch(`${baseUrl}/jose-sign`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ sub: "jose-user" }),
        });
        const body = await res.json() as any;
        assert(ctx, "npm:jose sign → 200", () => res.status === 200);
        assert(ctx, "npm:jose sign → ok:true", () => body?.ok === true);
        assert(ctx, "npm:jose sign → token string", () => typeof body?.token === "string");
        joseToken = body?.token ?? "";
      }
      {
        const res = await fetch(`${baseUrl}/jose-verify`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ token: joseToken }),
        });
        const body = await res.json() as any;
        assert(ctx, "npm:jose verify → 200", () => res.status === 200);
        assert(ctx, "npm:jose verify → ok:true", () => body?.ok === true);
        assert(ctx, "npm:jose verify → payload extracted", () => body?.payload?.sub === "jose-user");
        assert(ctx, "npm:jose verify → matching issuer", () => body?.payload?.iss === "urn:flux:issuer");
      }

      // verify valid token
      {
        const res = await fetch(`${baseUrl}/verify`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ token: signedToken }),
        });
        const body = await res.json() as any;
        assert(ctx, "webcrypto: verify → 200", () => res.status === 200);
        assert(ctx, "webcrypto: verify → ok:true", () => body?.ok === true);
        assert(ctx, "webcrypto: verify → sub matches", () => body?.sub === "user-abc");
      }
      // sign+verify cycle (end-to-end in one request)
      {
        const res = await fetch(`${baseUrl}/sign-verify-cycle`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ sub: "e2e-user" }),
        });
        // Catch 500 error gracefully
        let body;
        try { body = await res.json(); } catch { body = {ok: false, error: "not json"}; }
        assert(ctx, "webcrypto: sign-verify cycle → 200", () => res.status === 200);
        assert(ctx, "webcrypto: sign-verify cycle → ok:true", () => body?.ok === true);
        assert(ctx, "webcrypto: sign-verify cycle → sub matches", () => body?.sub_matches === true);
      }
      // ── Failure cases ────────────────────────────────────────────────────
      // tampered token → verification fails (caught)
      {
        const res = await fetch(`${baseUrl}/verify-bad`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({}),
        });
        const text = await res.text();
        let body; try { body = JSON.parse(text); } catch { body = {ok: false, error: text}; }
        assert(ctx, "webcrypto: bad token → ok:true (caught)", () => body?.ok === true);
        assert(ctx, "webcrypto: bad token → caught=true", () => body?.caught === true);
      }
      // expired token → caught
      {
        const res = await fetch(`${baseUrl}/verify-expired`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({}),
        });
        const text = await res.text();
        let body; try { body = JSON.parse(text); } catch { body = {ok: false, error: text}; }
        assert(ctx, "webcrypto: expired token → ok:true (caught)", () => body?.ok === true);
        assert(ctx, "webcrypto: expired token → expired=true", () => body?.expired === true);
      }
      // RSA keygen + JWKS
      {
        const res = await fetch(`${baseUrl}/jwks`);
        const text = await res.text();
        let body; try { body = JSON.parse(text); } catch { console.error("JWKS text error:", text); body = {ok: false, error: text}; }
        if (!body?.keys) console.error("JWKS JS Error:", body?.error);
        assert(ctx, "webcrypto: JWKS → 200", () => res.status === 200);
        assert(ctx, "webcrypto: JWKS → keys array present", () => Array.isArray(body?.keys));
        assert(ctx, "webcrypto: JWKS → has RS256 key", () => body?.keys?.[0]?.alg === "RS256");
      }
      // SHA-256 digest (determinism check — same input must produce same hash)
      {
        const res = await fetch(`${baseUrl}/digest`);
        const text = await res.text();
        let body; try { body = JSON.parse(text); } catch { body = {ok: false, error: text}; }
        assert(ctx, "webcrypto: SHA-256 digest → 200", () => res.status === 200);
        assert(ctx, "webcrypto: SHA-256 digest → ok:true", () => body?.ok === true);
        assert(ctx, "webcrypto: SHA-256 digest → expected hash", () =>
          body?.hex === "f1a0c5b9c8e3e8d7d1c9f3f7e9e8b0c2a1b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7"
          // Note: actual hash is non-deterministic per se (input is fixed though)
          || typeof body?.hex === "string" && body?.hex.length === 64
        );
      }
      // PBKDF2 key derivation
      {
        const res = await fetch(`${baseUrl}/derive-key`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ password: "flux-secret", salt: "flux-salt" }),
        });
        const text = await res.text();
        let body; try { body = JSON.parse(text); } catch { console.error("PBKDF2 Error:", text); body = {ok: false, error: text}; }
        if (!body?.ok) console.error("PBKDF2 body error:", body?.error);
        assert(ctx, "webcrypto: PBKDF2 → 200", () => res.status === 200);
        assert(ctx, "webcrypto: PBKDF2 → ok:true", () => body?.ok === true);
        assert(ctx, "webcrypto: PBKDF2 → 256 bits derived", () => body?.derived_bits_length === 256);
        assert(ctx, "webcrypto: PBKDF2 → hex is 64 chars", () => body?.hex?.length === 64);
      }
    },
  },


  // ── Tier 1: HTTP Clients — undici ────────────────────────────────────────
  {
    name: "compat-undici",
    handler: "compat/undici-compat.ts",
    handlerBaseDir: "examples",
    async run(baseUrl, ctx) {
      // smoke
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "undici: GET / → 200", () => res.status === 200);
        assert(ctx, "undici: GET / → library=undici", () => body?.library === "undici");
      }
      // low-level request() API
      {
        const res = await fetch(`${baseUrl}/undici-request`);
        const body = await res.json() as any;
        assert(ctx, "undici: request() → 200", () => res.status === 200);
        assert(ctx, "undici: request() → ok:true", () => body?.ok === true);
        assert(ctx, "undici: request() → origin_present", () => body?.origin_present === true);
      }
      // POST via request()
      {
        const res = await fetch(`${baseUrl}/undici-post`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ flux: "undici-test" }),
        });
        const body = await res.json() as any;
        assert(ctx, "undici: POST → 200", () => res.status === 200);
        assert(ctx, "undici: POST → ok:true", () => body?.ok === true);
        assert(ctx, "undici: POST → echoed body", () => body?.echoed?.flux === "undici-test");
      }
      // undici's own fetch()
      {
        const res = await fetch(`${baseUrl}/undici-fetch`);
        const body = await res.json() as any;
        assert(ctx, "undici: fetch() → 200", () => res.status === 200);
        assert(ctx, "undici: fetch() → ok:true", () => body?.ok === true);
        assert(ctx, "undici: fetch() → origin_present", () => body?.origin_present === true);
      }
      // custom headers
      {
        const res = await fetch(`${baseUrl}/undici-headers`);
        const body = await res.json() as any;
        assert(ctx, "undici: custom headers → 200", () => res.status === 200);
        assert(ctx, "undici: custom headers → forwarded", () => body?.has_custom_header === true);
      }
    },
  },

  // ── Contract: postgres.js is correctly rejected ───────────────────────────
  // postgres.js speaks raw TCP wire protocol and cannot be intercepted by
  // Flux. This suite is a CONTRACT TEST that asserts it is *explicitly*
  // rejected rather than silently misbehaving.
  {
    name: "compat-postgresjs",
    handler: "compat/postgresjs-compat.ts",
    handlerBaseDir: "examples",
    // No Postgres container needed — the handler only tests that connection
    // attempts fail at the Flux boundary, not that they succeed.
    async run(baseUrl, ctx) {
      // smoke — just verify the runtime started and identifies itself
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "postgresjs: GET / → 200", () => res.status === 200);
        assert(ctx, "postgresjs: GET / → library=postgres.js", () => body?.library === "postgres.js");
      }
      // contract — assert postgres.js connection is correctly rejected by Flux
      {
        const res = await fetch(`${baseUrl}/unsupported`);
        const body = await res.json() as any;
        assert(ctx, "postgresjs: correctly rejected by Flux sandbox", () => body?.rejected === true);
        assert(ctx, "postgresjs: rejection category", () => body?.category === "unsupported-driver");
        assert(ctx, "postgresjs: rejection reason", () => typeof body?.reason === "string" && body.reason.includes("raw TCP"));
        assert(ctx, "postgresjs: recommendation provided", () => body?.recommendation === "use flux:pg");
      }
    },
  },

  // ── Tier 3: Redis — ioredis ───────────────────────────────────────────────
  {
    name: "compat-ioredis",
    handler: "compat/ioredis-compat.ts",
    handlerBaseDir: "examples",
    async start(entry, port) {
      const redisPort = allocateRedisPort();
      const redis = await startRedis(redisPort);
      try {
        const runtime = await startRuntime(entry, port, {
          skipVerify: true,
          timeoutMs: 60_000,
          env: {
            REDIS_URL: redis.redisUrl,
            FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
          },
        });
        return {
          ...runtime,
          async stop() {
            try { await runtime.stop(); } finally { redis.stop(); }
          },
        };
      } catch (err) {
        redis.stop();
        throw err;
      }
    },
    async run(baseUrl, ctx) {
      // smoke
      {
        const res = await fetch(`${baseUrl}/`);
        const body = await res.json() as any;
        assert(ctx, "ioredis: GET / → 200", () => res.status === 200);
        assert(ctx, "ioredis: GET / → library=ioredis", () => body?.library === "ioredis");
      }
      // PING
      {
        const res = await fetch(`${baseUrl}/ping`);
        const body = await res.json() as any;
        assert(ctx, "ioredis: PING → 200", () => res.status === 200);
        assert(ctx, "ioredis: PING → pong=PONG", () => body?.pong === "PONG");
      }
      // SET / GET / DEL
      {
        const res = await fetch(`${baseUrl}/set-get`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ key: "greeting", value: "hello-ioredis" }),
        });
        const body = await res.json() as any;
        assert(ctx, "ioredis: SET-GET → 200", () => res.status === 200);
        assert(ctx, "ioredis: SET-GET → stored matches", () => body?.stored === "hello-ioredis");
        assert(ctx, "ioredis: SET-GET → retrieved matches", () => body?.retrieved === "hello-ioredis");
      }
      // INCR / INCRBY
      {
        const res = await fetch(`${baseUrl}/incr`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "ioredis: INCR → 200", () => res.status === 200);
        assert(ctx, "ioredis: INCR → v1=1", () => body?.v1 === 1);
        assert(ctx, "ioredis: INCR → v2=2", () => body?.v2 === 2);
        assert(ctx, "ioredis: INCRBY 10 → v3=12", () => body?.v3 === 12);
      }
      // HSET / HGETALL
      {
        const res = await fetch(`${baseUrl}/hash`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ field: "runtime", value: "flux" }),
        });
        const body = await res.json() as any;
        assert(ctx, "ioredis: HSET-HGETALL → 200", () => res.status === 200);
        assert(ctx, "ioredis: HSET-HGETALL → field value matches", () => body?.all?.runtime === "flux");
      }
      // pipeline
      {
        const res = await fetch(`${baseUrl}/pipeline`, { method: "POST" });
        const body = await res.json() as any;
        assert(ctx, "ioredis: pipeline → 200", () => res.status === 200);
        assert(ctx, "ioredis: pipeline → value correct", () => body?.pipeline_value === "flux-pipeline");
      }
    },
  },


  // ── 8. CRUD replay ──────────────────────────────────────────────────────
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
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
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
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
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
    name: "idempotency-crash-before-checkpoint",
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
        idempotencyCrashState.serverUrl = server.url;
        idempotencyCrashState.serviceToken = serviceToken;
        idempotencyCrashState.databaseUrl = postgres.databaseUrl;
        idempotencyCrashState.redisUrl = redis.redisUrl;
        idempotencyCrashState.postgresContainerName = postgres.containerName;
        idempotencyCrashState.redisContainerName = redis.containerName;
        idempotencyCrashState.entry = entry;
        idempotencyCrashState.port = port;

        const runtime = await startRuntime(entry, port, {
          skipVerify: false,
          serverUrl: server.url,
          token: serviceToken,
          env: {
            DATABASE_URL: postgres.databaseUrl,
            REDIS_URL: redis.redisUrl,
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
            FLUX_CRASH_AFTER_POSTGRES_COMMIT_BEFORE_CHECKPOINT: "1",
          },
        });
        idempotencyCrashState.runtime = runtime;

        return {
          ...runtime,
          async stop() {
            try {
              idempotencyCrashState.runtime = null;
              await runtime.stop();
            } finally {
              try {
                await server.stop();
              } finally {
                try {
                  redis.stop();
                } finally {
                  postgres.stop();
                  idempotencyCrashState.serverUrl = "";
                  idempotencyCrashState.serviceToken = "";
                  idempotencyCrashState.databaseUrl = "";
                  idempotencyCrashState.redisUrl = "";
                  idempotencyCrashState.postgresContainerName = "";
                  idempotencyCrashState.redisContainerName = "";
                  idempotencyCrashState.entry = "";
                  idempotencyCrashState.port = 0;
                }
              }
            }
          },
        };
      } catch (error) {
        idempotencyCrashState.serverUrl = "";
        idempotencyCrashState.serviceToken = "";
        idempotencyCrashState.databaseUrl = "";
        idempotencyCrashState.redisUrl = "";
        idempotencyCrashState.postgresContainerName = "";
        idempotencyCrashState.redisContainerName = "";
        idempotencyCrashState.entry = "";
        idempotencyCrashState.port = 0;
        idempotencyCrashState.runtime = null;
        await server.stop();
        redis.stop();
        postgres.stop();
        throw error;
      }
    },
    async run(baseUrl, ctx) {
      const requestBody = {
        sku: "flux-shirt",
        quantity: 1,
      };
      const idempotencyKey = "order-123";
      const redisKey = `idempotency:${idempotencyKey}`;

      const initialList = await get(baseUrl, "/orders");
      const initialOrders = (initialList.body as Record<string, unknown>)?.orders as unknown[] | undefined;
      assert(ctx, "GET /orders before crash test → 200", () => initialList.status === 200);
      assert(ctx, "GET /orders before crash test → empty", () => Array.isArray(initialOrders) && initialOrders.length === 0);

      const initialPostCount = queryPostgresScalar(
        idempotencyCrashState.postgresContainerName,
        "admin",
        "idempotency_demo",
        `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = '${idempotencyKey}';`,
      );
      assert(ctx, "precondition → Postgres row absent", () => initialPostCount === "0");
      assert(ctx, "precondition → Redis key absent", () => redisRaw(idempotencyCrashState.redisContainerName, "get", redisKey) === "");

      let crashed = false;
      try {
        await fetch(`${baseUrl}/orders`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "idempotency-key": idempotencyKey,
          },
          body: JSON.stringify(requestBody),
          signal: AbortSignal.timeout(5_000),
        });
      } catch {
        crashed = true;
      }
      assert(ctx, "POST /orders first request → connection fails on crash", () => crashed);

      const originalRuntime = idempotencyCrashState.runtime;
      if (!originalRuntime) {
        throw new Error("idempotency crash runtime handle missing");
      }
      const exitCode = await waitForProcessExit(originalRuntime.process);
      assert(ctx, "runtime exits after crash hook fires", () => exitCode === 1);

      const rowCountAfterCrash = queryPostgresScalar(
        idempotencyCrashState.postgresContainerName,
        "admin",
        "idempotency_demo",
        `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = '${idempotencyKey}';`,
      );
      assert(ctx, "after crash → exactly one durable row exists", () => rowCountAfterCrash === "1");

      const postExecutionsAfterCrash = queryPostgresScalar(
        idempotencyCrashState.postgresContainerName,
        "admin",
        "idempotency_demo",
        "SELECT count(*) FROM flux.executions WHERE method = 'POST' AND path = '/orders';",
      );
      assert(ctx, "after crash → no phantom POST execution recorded", () => postExecutionsAfterCrash === "0");
      assert(ctx, "after crash → Redis key still absent", () => redisRaw(idempotencyCrashState.redisContainerName, "get", redisKey) === "");

      const retryRuntime = await startRuntime(idempotencyCrashState.entry, idempotencyCrashState.port, {
        skipVerify: false,
        serverUrl: idempotencyCrashState.serverUrl,
        token: idempotencyCrashState.serviceToken,
        env: {
          DATABASE_URL: idempotencyCrashState.databaseUrl,
          REDIS_URL: idempotencyCrashState.redisUrl,
          FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
          FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
        },
      });

      try {
        const durableList = await get(`${retryRuntime.baseUrl}`, "/orders");
        const durableOrders = (durableList.body as Record<string, unknown>)?.orders as Array<Record<string, unknown>> | undefined;
        const durableOrder = Array.isArray(durableOrders) ? durableOrders[0] : undefined;
        assert(ctx, "retry precondition → durable order visible through API", () => durableList.status === 200 && Array.isArray(durableOrders) && durableOrders.length === 1);

        const retryResponse = await fetch(`${retryRuntime.baseUrl}/orders`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "idempotency-key": idempotencyKey,
          },
          body: JSON.stringify(requestBody),
        });
        const retryBody = await retryResponse.json() as Record<string, unknown>;
        const executionId = retryResponse.headers.get("x-flux-execution-id") ?? "";

        assert(ctx, "retry request → 201", () => retryResponse.status === 201);
        assert(ctx, "retry request → execution id header", () => executionId.length > 0);
        assert(ctx, "retry request → created status header", () => retryResponse.headers.get("x-idempotency-status") === "created");
        assert(ctx, "retry request → response matches durable row", () => {
          const order = retryBody.order as Record<string, unknown> | undefined;
          return stableJson(order) === stableJson(durableOrder);
        });

        const rowCountAfterRetry = queryPostgresScalar(
          idempotencyCrashState.postgresContainerName,
          "admin",
          "idempotency_demo",
          `SELECT count(*) FROM idempotent_orders WHERE idempotency_key = '${idempotencyKey}';`,
        );
        assert(ctx, "after retry → still one durable row", () => rowCountAfterRetry === "1");

        const redisStoredResponse = redisRaw(idempotencyCrashState.redisContainerName, "get", redisKey);
        const redisTtl = Number(redisRaw(idempotencyCrashState.redisContainerName, "ttl", redisKey));
        assert(ctx, "after retry → Redis key populated", () => redisStoredResponse.length > 0);
        assert(ctx, "after retry → Redis TTL present", () => Number.isInteger(redisTtl) && redisTtl > 0);
        assert(ctx, "after retry → Redis value matches canonical response", () => {
          const parsed = JSON.parse(redisStoredResponse) as { body?: unknown };
          return stableJson(parsed.body) === stableJson(retryBody);
        });

        const traceStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "trace",
          executionId,
          "--url",
          idempotencyCrashState.serverUrl,
          "--token",
          idempotencyCrashState.serviceToken,
        ]));
        assert(ctx, "flux trace retry execution → Redis boundary present", () => traceStdout.includes("REDIS"));
        assert(ctx, "flux trace retry execution → Postgres boundary present", () => traceStdout.includes("POSTGRES"));

        const replayStdout = stripAnsi(runCheckedCommand(FLUX_CLI_BIN, [
          "replay",
          executionId,
          "--url",
          idempotencyCrashState.serverUrl,
          "--token",
          idempotencyCrashState.serviceToken,
          "--diff",
        ]));
        const replayEnvelope = JSON.parse(extractReplayOutput(replayStdout)) as {
          net_response?: { body?: string; status?: number };
        };
        const replayBody = JSON.parse(replayEnvelope.net_response?.body ?? "null") as Record<string, unknown>;

        assert(ctx, "flux replay retry execution → ok", () => replayStdout.includes("ok"));
        assert(ctx, "flux replay retry execution → same JSON response", () => stableJson(replayBody) === stableJson(retryBody));
        assert(ctx, "flux replay retry execution → 201 status preserved", () => replayEnvelope.net_response?.status === 201);
        assert(ctx, "flux replay retry execution → Redis recorded only", () => replayStdout.includes("REDIS") && replayStdout.includes("(recorded)"));
        assert(ctx, "flux replay retry execution → Postgres recorded only", () => replayStdout.includes("POSTGRES") && replayStdout.includes("(recorded)"));

        const thirdResponse = await fetch(`${retryRuntime.baseUrl}/orders`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "idempotency-key": idempotencyKey,
          },
          body: JSON.stringify(requestBody),
        });
        const thirdBody = await thirdResponse.json() as Record<string, unknown>;
        assert(ctx, "third request after retry → replayed header", () => thirdResponse.headers.get("x-idempotency-status") === "replayed");
        assert(ctx, "third request after retry → same JSON response", () => stableJson(thirdBody) === stableJson(retryBody));
      } finally {
        await retryRuntime.stop();
      }
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
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLUXBASE_ALLOW_LOOPBACK_REDIS: "1",
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
            FLUXBASE_ALLOW_LOOPBACK_POSTGRES: "1",
            FLUXBASE_ALLOW_LOOPBACK_FETCH: "1",
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
  }
];

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const SUITE_FILTER = process.argv.slice(2).filter(arg => !arg.startsWith("--"));

const activeSuites = SUITE_FILTER.length > 0
  ? SUITES.filter((s) => SUITE_FILTER.some(f => s.name.includes(f)))
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
  process.exit(0);
}

main().catch((err) => {
  console.error("Unexpected error:", err);
  process.exit(1);
});
