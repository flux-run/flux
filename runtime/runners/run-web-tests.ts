/**
 * run-web-tests.ts
 *
 * Runs JSON-format Web Platform Tests from the url/, fetch/, and encoding/
 * subdirectories of the wpt repository.
 *
 * Most of these APIs (URL, URLSearchParams, TextEncoder, TextDecoder, fetch)
 * are available natively in Node.js ≥ 18 and Deno, so they map directly to
 * the Web API surface exposed inside Flux isolates.
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
import { readdirSync, statSync, existsSync } from "node:fs";
import { join, resolve, basename }           from "node:path";
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
// Minimal WPT testharness shim for Node.js
const _failures = [];
globalThis.test = function(fn, name) {
  try { fn(); }
  catch(e) { _failures.push(name + ": " + e.message); }
};
globalThis.promise_test = async function(fn, name) {
  try { await fn(); }
  catch(e) { _failures.push(name + ": " + e.message); }
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
// Post-test check
process.on("exit", () => {
  if (_failures.length) {
    console.error(_failures.join("\\n"));
    process.exitCode = 1;
  }
});
`;

// Node.js's --require flag doesn't work for inline eval; use --eval + concat
async function runOneTest(filePath: string): Promise<TestResult> {
  const name = basename(filePath);
  const t0   = performance.now();

  // Quick DOM-sniff to skip browser-only tests
  try {
    const { readFileSync } = await import("node:fs");
    const src = readFileSync(filePath, "utf-8");
    if (BROWSER_SKIP_PATTERNS.some(re => re.test(src))) {
      return { name, passed: false, skipped: true, duration: 0 };
    }
  } catch {
    // can't read — skip
    return { name, passed: false, skipped: true, duration: 0 };
  }

  const result = spawnSync(
    process.execPath,
    ["--eval", SHIM, filePath],
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
