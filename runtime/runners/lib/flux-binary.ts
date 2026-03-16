/**
 * lib/flux-binary.ts
 *
 * Helpers for finding, building, and lifecycle-managing the flux-runtime binary
 * during integration tests.
 *
 * The binary is expected at:
 *   <workspace>/target/debug/flux-runtime   (debug, default)
 *   <workspace>/target/release/flux-runtime (release, when FLUX_RELEASE=1)
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
/** Path to the compiled flux-runtime binary. */
export const FLUX_RUNTIME_BIN = resolve(WORKSPACE_ROOT, "target", PROFILE, "flux-runtime");

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/** Returns true if the binary already exists on disk. */
export function binaryExists(): boolean {
  return existsSync(FLUX_RUNTIME_BIN);
}

/**
 * Compiles the flux-runtime binary via `cargo build`.
 * Throws if the build fails.
 */
export function buildFlux(opts: { release?: boolean; quiet?: boolean } = {}): void {
  const profile = opts.release ? ["--release"] : [];
  const cmd     = ["build", "-p", "runtime", ...profile];

  console.log(`  Building flux-runtime (${opts.release ? "release" : "debug"})…`);

  const result = spawnSync("cargo", cmd, {
    cwd:   WORKSPACE_ROOT,
    stdio: opts.quiet ? "pipe" : "inherit",
    env:   { ...process.env, SQLX_OFFLINE: "true" },
  });

  if (result.status !== 0) {
    const stderr = result.stderr?.toString() ?? "";
    throw new Error(`cargo build failed (exit ${result.status})\n${stderr}`);
  }

  console.log(`  ✓ flux-runtime built → ${FLUX_RUNTIME_BIN}`);
}

/**
 * Ensures the binary exists, building it first if needed (or if `force` is set).
 * Pass `--skip-build` on the CLI to bypass this check entirely.
 */
export function ensureBinary(opts: { force?: boolean; quiet?: boolean } = {}): void {
  if (process.argv.includes("--skip-build")) {
    if (!binaryExists()) {
      throw new Error(
        `--skip-build was set but binary not found at:\n  ${FLUX_RUNTIME_BIN}\n` +
        `Run: cd ${WORKSPACE_ROOT} && SQLX_OFFLINE=true cargo build -p runtime`,
      );
    }
    return;
  }

  if (opts.force || !binaryExists()) {
    buildFlux({ quiet: opts.quiet });
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
 * Start flux-runtime with the given entry file.
 *
 * @param entryAbsPath  Absolute path to the JS handler file to serve.
 * @param port          HTTP port to listen on (default: 3100, outside the normal 3000).
 * @param opts          Extra configuration.
 */
export async function startRuntime(
  entryAbsPath: string,
  port = 3100,
  opts: {
    host?:           string;
    isolatePoolSize?: number;
    timeoutMs?:      number;
    token?:          string;
  } = {},
): Promise<RuntimeHandle> {
  const host           = opts.host            ?? "127.0.0.1";
  const isolatePoolSize = opts.isolatePoolSize ?? 1;
  const token          = opts.token           ?? "test-token";
  const timeoutMs      = opts.timeoutMs       ?? 15_000;

  const args = [
    "--entry", entryAbsPath,
    "--host",  host,
    "--port",  String(port),
    "--isolate-pool-size", String(isolatePoolSize),
  ];

  const proc = spawn(FLUX_RUNTIME_BIN, args, {
    cwd: WORKSPACE_ROOT,
    env: { ...process.env, FLUX_SERVICE_TOKEN: token },
    stdio: ["ignore", "pipe", "pipe"],
  });

  proc.on("error", (err) => {
    throw new Error(`flux-runtime failed to start: ${err.message}`);
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
