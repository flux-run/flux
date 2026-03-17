/**
 * run-web-tests.ts
 *
 * Runs Web Platform Tests (url / fetch / encoding) via `flux run` to measure
 * Web API compatibility inside Flux's V8 isolates.
 *
 * Prerequisites
 * -------------
 *   bash runtime/scripts/setup-external-tests.sh wpt
 *     (or)
 *   cd runtime/external-tests
 *   git clone --depth 1 --filter=blob:none --sparse \
 *             https://github.com/web-platform-tests/wpt web-platform
 *   cd web-platform && git sparse-checkout set url fetch encoding
 *
 * Usage
 * -----
 *   npm run test:web
 *   npm run test:web -- --suite url          # run only url/ tests
 *   npm run test:web -- --suite fetch
 *
 * Output
 * ------
 *   runtime/reports/web-tests.json
 */

import { spawnSync }    from "node:child_process";
import { readdirSync, statSync, existsSync, readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { join, resolve, basename, dirname }  from "node:path";
import { tmpdir }                            from "node:os";
import { FLUX_CLI_BIN }                      from "./lib/flux-binary.js";
import { performance }  from "node:perf_hooks";
import {
  EXTERNAL_TESTS_DIR,
  TestResult,
  buildReport,
  writeReport,
  printSummary,
  requireDirectory,
} from "./lib/utils.js";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const WPT_DIR = resolve(EXTERNAL_TESTS_DIR, "web-platform");

/** Only these subdirectories are relevant for a serverless JS runtime. */
const TARGET_SUITES = ["url", "fetch", "encoding"] as const;

/** Patterns in test content that indicate a browser-only test. */
const BROWSER_SKIP_PATTERNS = [
  /document\./,
  /window\./,
  /navigator\./,
  /location\./,
  /history\./,
  /localStorage/,
  /sessionStorage/,
  /ServiceWorker/,
  /importScripts/,
  /self\.postMessage/,
];

/** Patterns that require WPT server plumbing or browser-only helpers we do not provide. */
const HARNESS_SKIP_PATTERNS = [
  /idl_test\s*\(/,
  /get_host_info\s*\(/,
  /\bRESOURCES_DIR\b/,
  /\brequestForbiddenHeaders\b/,
  /META:\s*script=\/common\/get-host-info\.sub\.js/,
  /META:\s*script=\/common\/utils\.js/,
  /META:\s*script=\/common\/dispatcher\/dispatcher\.js/,
];

const suiteArg = process.argv.indexOf("--suite");
const SUITE    = suiteArg !== -1 ? process.argv[suiteArg + 1] : undefined;

// Temp directory for writing SHIM + test source so flux run can execute both.
// Sequential execution means we can safely reuse a single file.
const TEMP_DIR = mkdtempSync(join(tmpdir(), "flux-wpt-"));
process.on("exit", () => rmSync(TEMP_DIR, { recursive: true, force: true }));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function collectJs(dir: string, out: string[] = []): string[] {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st   = statSync(full);
    if (st.isDirectory()) {
      collectJs(full, out);
    } else if (entry.endsWith(".js") || entry.endsWith(".any.js")) {
      out.push(full);
    }
  }
  return out;
}

function resolveMetaScript(testFilePath: string, scriptPath: string): string {
  if (scriptPath.startsWith("/")) {
    return join(WPT_DIR, scriptPath.slice(1));
  }

  return join(dirname(testFilePath), scriptPath);
}

function loadMetaScripts(filePath: string, src: string, visited = new Set<string>()): string[] {
  const metaScriptPaths = [...src.matchAll(/^\/\/ META:\s*script=(.+)$/gm)]
    .map((match) => match[1]?.trim())
    .filter((value): value is string => Boolean(value));

  const loaded: string[] = [];

  for (const scriptPath of metaScriptPaths) {
    const resolvedPath = resolveMetaScript(filePath, scriptPath);
    if (visited.has(resolvedPath) || !existsSync(resolvedPath)) {
      continue;
    }

    visited.add(resolvedPath);
    const scriptSource = readFileSync(resolvedPath, "utf-8");
    loaded.push(...loadMetaScripts(resolvedPath, scriptSource, visited));
    loaded.push(scriptSource);
  }

  return loaded;
}

function resolveFixtureAsset(testFilePath: string, assetPath: string): string | null {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(assetPath) || assetPath.startsWith("//")) {
    return null;
  }

  const resolvedPath = join(dirname(testFilePath), assetPath);
  return existsSync(resolvedPath) ? resolvedPath : null;
}

function detectContentType(assetPath: string): string {
  if (assetPath.endsWith(".json")) return "application/json";
  if (assetPath.endsWith(".txt")) return "text/plain";
  if (assetPath.endsWith(".html")) return "text/html";
  if (assetPath.endsWith(".js")) return "text/javascript";
  return "application/octet-stream";
}

function buildFetchAssetShim(testFilePath: string, sources: string[]): string {
  const assets = new Map<string, { contentType: string; body: string }>();

  for (const source of sources) {
    for (const match of source.matchAll(/fetch\(\s*(["'`])([^"'`]+)\1/g)) {
      const assetPath = match[2];
      if (!assetPath) continue;
      const resolvedPath = resolveFixtureAsset(testFilePath, assetPath);
      if (!resolvedPath || assets.has(assetPath)) continue;
      assets.set(assetPath, {
        contentType: detectContentType(assetPath),
        body: readFileSync(resolvedPath, "utf-8"),
      });
    }
  }

  const serializedAssets = JSON.stringify([...assets.entries()]);

  return `
const __wptAssetMap = new Map(${serializedAssets});
const __wptOriginalFetch = globalThis.fetch ? globalThis.fetch.bind(globalThis) : null;
globalThis.fetch = async function(input, init = undefined) {
  const rawUrl = input instanceof Request ? input.url : String(input);
  const asset = __wptAssetMap.get(rawUrl);
  if (asset) {
    return new Response(asset.body, {
      status: 200,
      headers: { "content-type": asset.contentType },
    });
  }

  if (!__wptOriginalFetch) {
    throw new TypeError("fetch is not available");
  }

  return __wptOriginalFetch(input, init);
};
`;
}

// ---------------------------------------------------------------------------
// Per-test wrapper
//
// WPT .any.js tests are designed for a custom harness. We run each file via
// Node.js with a minimal shim that stubs `test()` / `assert_*` / `promise_test`
// (the wpt testharness.js API surface) so the assertions execute natively.
// ---------------------------------------------------------------------------

const SHIM = `
// Minimal WPT testharness shim — no Node.js process globals (runs in Flux isolate)
const _failures = [];
const _pending = [];
const _cleanups = [];

function _formatError(error) {
  if (error && typeof error.message === "string") return error.message;
  if (typeof error === "string") return error;
  if (error === undefined) return "undefined";
  try { return JSON.stringify(error); }
  catch { return String(error); }
}

function _recordFailure(name, error) {
  _failures.push((name || "unknown") + ": " + _formatError(error));
}

function _makeTestContext() {
  return {
    add_cleanup(fn) {
      if (typeof fn === "function") _cleanups.push(fn);
    },
    step(fn, label) {
      try { return fn(); }
      catch (error) { _recordFailure(label, error); }
    },
    step_timeout(fn, _ms) {
      return Promise.resolve().then(() => fn());
    },
    unreachable_func(message) {
      return () => { throw new Error(message || "unreachable"); };
    },
  };
}

globalThis.self = globalThis;
globalThis.window = globalThis;
globalThis.global = globalThis;

globalThis.test = function(fn, name) {
  try { fn(_makeTestContext()); }
  catch(error) { _recordFailure(name, error); }
};
globalThis.promise_test = async function(fn, name) {
  const pending = Promise.resolve()
    .then(() => fn(_makeTestContext()))
    .catch((error) => _recordFailure(name, error));
  _pending.push(pending);
  return pending;
};
globalThis.assert_equals          = (a, b, msg) => { if (a !== b) throw new Error(msg || \`\${a} !== \${b}\`); };
globalThis.assert_not_equals      = (a, b, msg) => { if (a === b) throw new Error(msg || \`\${a} === \${b}\`); };
globalThis.assert_true            = (v, msg) => { if (!v) throw new Error(msg || "expected true"); };
globalThis.assert_false           = (v, msg) => { if (v)  throw new Error(msg || "expected false"); };
globalThis.assert_throws_js       = (Ctor, fn) => {
  try { fn(); throw new Error("expected throw"); }
  catch(e) { if (!(e instanceof Ctor)) throw new Error(\`Expected \${Ctor.name} but got \${e.constructor.name}\`); }
};
globalThis.assert_throws_exactly  = (err, fn) => {
  try { fn(); throw new Error("expected throw"); }
  catch(e) { if (e !== err) throw new Error("wrong error thrown"); }
};
globalThis.assert_unreached        = (msg) => { throw new Error(msg || "unreachable"); };
globalThis.assert_array_equals     = (a, b, msg) => {
  if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(msg || \`\${JSON.stringify(a)} !== \${JSON.stringify(b)}\`);
};
globalThis.assert_implements       = (v, msg) => { if (!v) throw new Error(msg || "not implemented"); };
globalThis.assert_greater_than     = (a, b, msg) => { if (!(a > b)) throw new Error(msg || \`\${a} is not greater than \${b}\`); };
globalThis.assert_less_than        = (a, b, msg) => { if (!(a < b)) throw new Error(msg || \`\${a} is not less than \${b}\`); };
globalThis.assert_less_than_equal  = (a, b, msg) => { if (!(a <= b)) throw new Error(msg || \`\${a} is not less than or equal to \${b}\`); };
globalThis.assert_greater_than_equal = (a, b, msg) => { if (!(a >= b)) throw new Error(msg || \`\${a} is not greater than or equal to \${b}\`); };
globalThis.assert_class_string     = () => {};
globalThis.format_value            = (value) => {
  try { return JSON.stringify(value); }
  catch { return String(value); }
};
globalThis.setup                   = () => {};
globalThis.done                    = () => {};
globalThis.add_result_callback     = () => {};
globalThis.subsetTest              = (testFn, fn, name) => testFn(fn, name);
globalThis.subsetTestByKey         = (_key, testFn, fn, name) => testFn(fn, name);
globalThis.promise_rejects_js      = async (_test, Ctor, promiseLike, description) => {
  try {
    await promiseLike;
  } catch (error) {
    if (error instanceof Ctor) {
      return;
    }
    throw new Error(description || \`Expected \${Ctor.name} but got \${error?.constructor?.name || typeof error}\`);
  }
  throw new Error(description || \`Expected promise to reject with \${Ctor.name}\`);
};
globalThis.promise_rejects_exactly = async (_test, expected, promiseLike, description) => {
  try {
    await promiseLike;
  } catch (error) {
    if (error === expected) {
      return;
    }
    throw new Error(description || "wrong rejection value");
  }
  throw new Error(description || "Expected promise to reject");
};
globalThis.__wpt_check_failures = async function() {
  await Promise.allSettled(_pending);
  for (const cleanup of _cleanups.splice(0)) {
    await cleanup();
  }
  if (_failures.length) throw new Error(_failures.join("\\n"));
};
`;

// Node.js's --require flag doesn't work for inline eval; use --eval + concat
async function runOneTest(filePath: string): Promise<TestResult> {
  const name = basename(filePath);
  const t0   = performance.now();

  // Read test source upfront — used for browser-sniff and SHIM concatenation
  let src: string;
  try {
    src = readFileSync(filePath, "utf-8");
    if (BROWSER_SKIP_PATTERNS.some(re => re.test(src)) || HARNESS_SKIP_PATTERNS.some(re => re.test(src))) {
      return { name, passed: false, skipped: true, duration: 0 };
    }
  } catch {
    // can't read — skip
    return { name, passed: false, skipped: true, duration: 0 };
  }

  const metaScripts = loadMetaScripts(filePath, src);
  const assetShim = buildFetchAssetShim(filePath, [...metaScripts, src]);
  const wrapperDir = dirname(filePath);
  const tmpFile = join(
    wrapperDir,
    `.flux-wpt-${process.pid}-${Date.now()}-${name.replace(/[^a-zA-Z0-9_.-]/g, "_")}.mjs`,
  );
  const wrapperSource = [
    SHIM,
    assetShim,
    ...metaScripts,
    src,
    "await globalThis.__wpt_check_failures();",
  ].join("\n\n");
  writeFileSync(tmpFile, wrapperSource, "utf-8");

  const result = spawnSync(
    FLUX_CLI_BIN,
    ["run", tmpFile],
    { timeout: 8000, encoding: "utf-8" },
  );

  rmSync(tmpFile, { force: true });

  const passed = result.status === 0 && !result.error;
  return {
    name,
    passed,
    skipped: false,
    error: passed ? undefined : (result.stderr || result.error?.message || "non-zero exit").slice(0, 300),
    duration: Math.round(performance.now() - t0),
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  if (!requireDirectory(WPT_DIR,
    "bash runtime/scripts/setup-external-tests.sh wpt")) {
    process.exit(1);
  }

  const suites = SUITE
    ? TARGET_SUITES.filter(s => s === SUITE)
    : [...TARGET_SUITES];

  console.log("\nWeb Platform Tests");
  console.log(`Suites: ${suites.join(", ")}\n`);

  const all: TestResult[] = [];
  const start = performance.now();

  for (const suite of suites) {
    const dir   = join(WPT_DIR, suite);
    const files = collectJs(dir);
    console.log(`  ${suite}/  — ${files.length} files`);

    for (const f of files) {
      all.push(await runOneTest(f));
    }
  }

  console.log();
  const report = buildReport("Web Platform Tests (url · fetch · encoding)", all, performance.now() - start);
  printSummary({
    ...report,
    results: all.filter(r => !r.passed && !r.skipped).slice(0, 20),
  });

  await writeReport("web-tests.json", report);
  if (report.failed > 0) process.exit(1);
}

main().catch((e) => { console.error(e); process.exit(1); });
