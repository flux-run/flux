/**
 * lib/flux-binary.ts
 *
 * Helpers for finding, building, and lifecycle-managing the Flux CLI
 * during integration tests.
 *
 * Tests use `flux serve --skip-verify <entry>` — the same command a developer
 * runs locally — rather than calling flux-runtime directly.
 *
 * Binaries expected at:
 *   <workspace>/target/debug/flux        (CLI, default)
 *   <workspace>/target/debug/flux-runtime (started by `flux serve` internally)
 *   <workspace>/target/release/…         (when FLUX_RELEASE=1)
 */

import { spawnSync, spawn, ChildProcess } from "node:child_process";
import { existsSync }                      from "node:fs";
import { resolve, dirname }                from "node:path";
import { fileURLToPath }                   from "node:url";
import { setTimeout as sleep }             from "node:timers/promises";
import { createConnection }                from "node:net";

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const __dirname    = dirname(fileURLToPath(import.meta.url));
/** Absolute path to the Cargo workspace root (two levels up from runners/lib). */
export const WORKSPACE_ROOT = resolve(__dirname, "../../..");
const PROFILE = process.env["FLUX_RELEASE"] === "1" ? "release" : "debug";
/** Path to the compiled flux CLI binary. */
export const FLUX_CLI_BIN     = resolve(WORKSPACE_ROOT, "target", PROFILE, "flux");
/** Path to the compiled flux-runtime binary (used by flux serve internally). */
export const FLUX_RUNTIME_BIN = resolve(WORKSPACE_ROOT, "target", PROFILE, "flux-runtime");
/** Path to the compiled flux-server binary. */
export const FLUX_SERVER_BIN  = resolve(WORKSPACE_ROOT, "target", PROFILE, "flux-server");

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/** Returns true if the CLI, runtime, and server binaries exist on disk. */
export function binaryExists(): boolean {
  return existsSync(FLUX_CLI_BIN) && existsSync(FLUX_RUNTIME_BIN) && existsSync(FLUX_SERVER_BIN);
}

/**
 * Compiles the `flux`, `flux-runtime`, and `flux-server` binaries via `cargo build`.
 * Throws if the build fails.
 */
export function buildFlux(opts: { release?: boolean; quiet?: boolean } = {}): void {
  const profile = opts.release ? ["--release"] : [];
  // Build the operator CLI, runtime, and recording server in one pass.
  const cmd = ["build", "-p", "cli", "-p", "runtime", "-p", "server", ...profile];

  console.log(`  Building flux CLI + runtime + server (${opts.release ? "release" : "debug"})…`);

  const result = spawnSync("cargo", cmd, {
    cwd:   WORKSPACE_ROOT,
    stdio: opts.quiet ? "pipe" : "inherit",
    env:   { ...process.env, SQLX_OFFLINE: "true" },
  });

  if (result.status !== 0) {
    const stderr = result.stderr?.toString() ?? "";
    throw new Error(`cargo build failed (exit ${result.status})\n${stderr}`);
  }

  console.log(`  ✓ flux built → ${FLUX_CLI_BIN}`);
  console.log(`  ✓ flux-runtime built → ${FLUX_RUNTIME_BIN}`);
  console.log(`  ✓ flux-server built → ${FLUX_SERVER_BIN}`);
}

/**
 * Ensures both binaries exist, building them if needed (or if `force` is set).
 * Pass `--skip-build` on the CLI to bypass this check entirely.
 */
export function ensureBinary(opts: { force?: boolean; quiet?: boolean } = {}): void {
  if (process.argv.includes("--skip-build")) {
    if (!binaryExists()) {
      throw new Error(
        `--skip-build was set but binaries not found.\n` +
        `Expected:\n  ${FLUX_CLI_BIN}\n  ${FLUX_RUNTIME_BIN}\n  ${FLUX_SERVER_BIN}\n` +
        `Run: cd ${WORKSPACE_ROOT} && SQLX_OFFLINE=true cargo build -p cli -p runtime -p server`,
      );
    }
    return;
  }

  if (opts.force || !binaryExists()) {
    buildFlux({ quiet: opts.quiet });
  }
}

// ---------------------------------------------------------------------------
// Artifact build
// ---------------------------------------------------------------------------

/**
 * Builds a Flux artifact for the given entry using the real `flux build` CLI.
 * Throws if the build fails.
 */
export function buildArtifact(entryAbsPath: string, opts: { quiet?: boolean } = {}): void {
  const result = spawnSync(FLUX_CLI_BIN, ["build", entryAbsPath], {
    cwd: WORKSPACE_ROOT,
    stdio: opts.quiet ? "pipe" : "inherit",
    env: { ...process.env, SQLX_OFFLINE: "true" },
  });

  if (result.status !== 0) {
    const stderr = result.stderr?.toString() ?? "";
    throw new Error(`flux build failed for ${entryAbsPath} (exit ${result.status})\n${stderr}`);
  }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

export interface RuntimeHandle {
  process: ChildProcess;
  host:    string;
  port:    number;
  baseUrl: string;
  /** Stop the runtime and wait for it to exit. */
  stop(): Promise<void>;
}

/**
 * Start a handler via `flux run --listen --skip-verify <entry>`.
 *
 * This is exactly the same command a developer runs locally, which means
 * the integration tests exercise the CLI's argument handling, binary lookup,
 * and the runtime — the full user-facing path.
 *
 * @param entryAbsPath  Absolute path to the JS handler file to serve.
 * @param port          HTTP port to listen on (default: 3100).
 * @param opts          Extra configuration.
 */
export async function startRuntime(
  entryAbsPath: string,
  port = 3100,
  opts: {
    host?:            string;
    isolatePoolSize?: number;
    timeoutMs?:       number;
    token?:           string;
    serverUrl?:       string;
    skipVerify?:      boolean;
    env?:             NodeJS.ProcessEnv;
  } = {},
): Promise<RuntimeHandle> {
  const host            = opts.host            ?? "127.0.0.1";
  const isolatePoolSize = opts.isolatePoolSize ?? 1;
  const token           = opts.token           ?? "test-token";
  const timeoutMs       = opts.timeoutMs       ?? 15_000;
  const skipVerify      = opts.skipVerify      ?? true;

  const args = [
    "run",
    "--listen",
    "--host",  host,
    "--port",  String(port),
    "--isolate-pool-size", String(isolatePoolSize),
  ];
  if (skipVerify) {
    args.push("--skip-verify");
  }
  if (opts.serverUrl) {
    args.push("--url", opts.serverUrl);
  }
  args.push(entryAbsPath);

  const proc = spawn(FLUX_CLI_BIN, args, {
    cwd: WORKSPACE_ROOT,
    // FLUX_SERVICE_TOKEN is read by `flux run` via clap's env() attribute
    env: { ...process.env, ...opts.env, FLUX_SERVICE_TOKEN: token },
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (process.env.VERBOSE && proc.stderr) {
    proc.stderr.on("data", (chunk) => process.stderr.write(chunk));
  }

  proc.on("error", (err) => {
    throw new Error(`flux run --listen failed to start: ${err.message}`);
  });

  // Wait until the port is open (or timeout)
  await waitForPort(host, port, timeoutMs);

  const baseUrl = `http://${host}:${port}`;

  return {
    process: proc,
    host,
    port,
    baseUrl,
    async stop() {
      proc.kill("SIGTERM");
      // Give the process up to 3 seconds to exit cleanly
      await Promise.race([
        new Promise<void>((resolve) => proc.once("exit", () => resolve())),
        sleep(3000),
      ]);
      if (!proc.killed) proc.kill("SIGKILL");
    },
  };
}

export interface ServerHandle {
  process: ChildProcess;
  host: string;
  port: number;
  url: string;
  stop(): Promise<void>;
}

export async function startServer(
  port = 50051,
  opts: {
    host?: string;
    databaseUrl: string;
    serviceToken: string;
    timeoutMs?: number;
    env?: NodeJS.ProcessEnv;
  },
): Promise<ServerHandle> {
  const host = opts.host ?? "127.0.0.1";
  const timeoutMs = opts.timeoutMs ?? 15_000;

  const proc = spawn(FLUX_SERVER_BIN, [], {
    cwd: WORKSPACE_ROOT,
    env: {
      ...process.env,
      ...opts.env,
      GRPC_PORT: String(port),
      DATABASE_URL: opts.databaseUrl,
      INTERNAL_SERVICE_TOKEN: opts.serviceToken,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  proc.on("error", (err) => {
    throw new Error(`flux-server failed to start: ${err.message}`);
  });

  await waitForPort(host, port, timeoutMs);

  return {
    process: proc,
    host,
    port,
    url: `http://${host}:${port}`,
    async stop() {
      proc.kill("SIGTERM");
      await Promise.race([
        new Promise<void>((resolve) => proc.once("exit", () => resolve())),
        sleep(3000),
      ]);
      if (!proc.killed) proc.kill("SIGKILL");
    },
  };
}

// ---------------------------------------------------------------------------
// Port-readiness polling
// ---------------------------------------------------------------------------

/**
 * Polls `host:port` every 100 ms until a TCP connection succeeds or the
 * timeout elapses (throws on timeout).
 */
async function waitForPort(host: string, port: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await isPortOpen(host, port)) return;
    await sleep(100);
  }
  throw new Error(
    `flux-runtime did not start within ${timeoutMs}ms on ${host}:${port}.\n` +
    `Binary: ${FLUX_RUNTIME_BIN}`,
  );
}

function isPortOpen(host: string, port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const sock = createConnection(port, host);
    sock.once("connect", () => { sock.destroy(); resolve(true); });
    sock.once("error",   () => { sock.destroy(); resolve(false); });
    sock.setTimeout(80, () => { sock.destroy(); resolve(false); });
  });
}

// ---------------------------------------------------------------------------
// Convenience: POST JSON and return parsed body
// ---------------------------------------------------------------------------

export async function postJson(
  baseUrl: string,
  path: string,
  body: unknown = {},
): Promise<{ status: number; body: unknown }> {
  const res = await fetch(`${baseUrl}${path}`, {
    method:  "POST",
    headers: { "content-type": "application/json" },
    body:    JSON.stringify(body),
  });
  let parsed: unknown;
  try { parsed = await res.json(); } catch { parsed = null; }
  return { status: res.status, body: parsed };
}

export async function get(
  baseUrl: string,
  path: string,
): Promise<{ status: number; body: unknown; text: string }> {
  const res  = await fetch(`${baseUrl}${path}`);
  const text = await res.text();
  let parsed: unknown;
  try { parsed = JSON.parse(text); } catch { parsed = null; }
  return { status: res.status, body: parsed, text };
}
