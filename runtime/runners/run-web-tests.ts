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
import { join, resolve, basename }           from "node:path";
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
globalThis.test = function(fn, name) {
  try { fn(); }
  catch(e) { _failures.push((name || "unknown") + ": " + e.message); }
};
globalThis.promise_test = async function(fn, name) {
  try { await fn(); }
  catch(e) { _failures.push((name || "unknown") + ": " + e.message); }
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
globalThis.assert_class_string     = () => {};
globalThis.setup                   = () => {};
globalThis.done                    = () => {};
globalThis.add_result_callback     = () => {};
// Checked synchronously after all top-level code runs (no process.on needed)
globalThis.__wpt_check_failures = function() {
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
    if (BROWSER_SKIP_PATTERNS.some(re => re.test(src))) {
      return { name, passed: false, skipped: true, duration: 0 };
    }
  } catch {
    // can't read — skip
    return { name, passed: false, skipped: true, duration: 0 };
  }

  // Write SHIM + test content + failure check to a temp file so `flux run` can execute both.
  const tmpFile = join(TEMP_DIR, "test.js");
  writeFileSync(tmpFile, SHIM + "\n" + src + "\n__wpt_check_failures();", "utf-8");

  const result = spawnSync(
    FLUX_CLI_BIN,
    ["run", tmpFile],
    { timeout: 8000, encoding: "utf-8" },
  );

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
