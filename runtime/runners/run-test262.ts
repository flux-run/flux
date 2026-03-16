/**
 * run-test262.ts
 *
 * Runs the TC39 Test262 conformance suite via `flux run` to measure
 * ECMAScript conformance inside Flux's V8 isolates.
 *
 * Prerequisites
 * -------------
 * 1. Clone test262:
 *      cd runtime/external-tests
 *      git clone --depth 1 https://github.com/tc39/test262 test262
 * 2. Install runner deps:
 *      cd runtime/runners && npm install
 *
 * Usage
 * -----
 *   npm run test:262              # run all non-excluded tests
 *   npm run test:262 -- --limit 500   # quick sample (first N tests)
 *
 * Output
 * ------
 *   runtime/reports/test262.json
 */

import { spawnSync }                                             from "node:child_process";
import { readFileSync, readdirSync, statSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { resolve, join, relative }                               from "node:path";
import { tmpdir }                                                from "node:os";
import { FLUX_CLI_BIN }                                          from "./lib/flux-binary.js";
import { performance }              from "node:perf_hooks";
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

const TEST262_DIR    = resolve(EXTERNAL_TESTS_DIR, "test262");
const TEST_DIR       = join(TEST262_DIR, "test");
const HARNESS_DIR    = join(TEST262_DIR, "harness");

/** Directories to exclude entirely (browser-only / optional features). */
const EXCLUDED_DIRS = new Set(["annexB", "intl402"]);

/** CLI flag  --limit <n>  caps how many tests are attempted (quick smoke run). */
const limitArg = process.argv.indexOf("--limit");
const LIMIT    = limitArg !== -1 ? parseInt(process.argv[limitArg + 1] ?? "9999", 10) : Infinity;

// Temp directory for writing combined test sources (harness + test body).
// Reuse a single file per test run (sequential execution, no concurrency).
const TEMP_DIR = mkdtempSync(join(tmpdir(), "flux-test262-"));
process.on("exit", () => rmSync(TEMP_DIR, { recursive: true, force: true }));

// Harness files loaded once and prepended to every test
const HARNESS_INLINE = ["assert.js", "sta.js", "doneprintHandle.js"]
  .map(f => {
    try { return readFileSync(join(HARNESS_DIR, f), "utf-8"); } catch { return ""; }
  })
  .join("\n");

// ---------------------------------------------------------------------------
// Test file discovery
// ---------------------------------------------------------------------------

function collectTests(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (EXCLUDED_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      collectTests(full, out);
    } else if (entry.endsWith(".js") && !entry.startsWith("_")) {
      out.push(full);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Per-test runner
// ---------------------------------------------------------------------------

interface YamlMeta {
  negative?: { phase?: string; type?: string };
  flags?:    string[];
  includes?: string[];
  features?: string[];
}

/** Very lightweight YAML front-matter parser (test262 uses a specific schema). */
function parseMeta(src: string): YamlMeta {
  const match = src.match(/\/\*---\n([\s\S]*?)---\*\//);
  if (!match) return {};
  const raw = match[1];
  const meta: YamlMeta = {};

  const neg = raw.match(/negative:\s*\n\s+phase:\s*(\S+)/);
  const typ = raw.match(/negative:\s*\n[\s\S]*?type:\s*(\S+)/);
  if (neg) meta.negative = { phase: neg[1], type: typ?.[1] };

  const flags = raw.match(/flags:\s*\[([^\]]*)\]/);
  if (flags) meta.flags = flags[1].split(",").map(s => s.trim()).filter(Boolean);

  const inc = raw.match(/includes:\s*\[([^\]]*)\]/);
  if (inc) meta.includes = inc[1].split(",").map(s => s.trim()).filter(Boolean);

  const feat = raw.match(/features:\s*\[([^\]]*)\]/);
  if (feat) meta.features = feat[1].split(",").map(s => s.trim()).filter(Boolean);

  return meta;
}

/** Browser-only features that should be skipped silently. */
const BROWSER_FEATURES = new Set([
  "Atomics.waitAsync", "ShadowRealm", "import-assertions", "import-attributes",
]);

function runOneTest(testPath: string): TestResult {
  const name  = relative(TEST_DIR, testPath);
  const t0    = performance.now();
  const src   = readFileSync(testPath, "utf-8");
  const meta  = parseMeta(src);

  // Skip module-flagged tests (Flux user code runs as scripts, not ESM)
  if (meta.flags?.includes("module") || meta.flags?.includes("async")) {
    return { name, passed: false, skipped: true, duration: 0 };
  }

  // Skip tests requiring browser-only features
  if (meta.features?.some(f => BROWSER_FEATURES.has(f))) {
    return { name, passed: false, skipped: true, duration: 0 };
  }

  // Build full test source: harness + extra includes + test body
  const extraIncludes = (meta.includes ?? [])
    .map(f => { try { return readFileSync(join(HARNESS_DIR, f), "utf-8"); } catch { return ""; } })
    .join("\n");

  const fullSrc = [HARNESS_INLINE, extraIncludes, src].join("\n");
  const isNegative = Boolean(meta.negative);

  const tmpFile = join(TEMP_DIR, "test.js");
  writeFileSync(tmpFile, fullSrc, "utf-8");
  const result = spawnSync(FLUX_CLI_BIN, ["run", tmpFile], {
    timeout:  5000,
    encoding: "utf-8",
  });

  const threw  = result.status !== 0;
  const passed = isNegative ? threw : !threw;
  const error  = !passed
    ? (isNegative ? "expected error but none thrown" : (result.stderr || result.error?.message || "failed"))
    : undefined;

  return {
    name,
    passed,
    skipped: false,
    error:   error?.slice(0, 200),
    duration: Math.round(performance.now() - t0),
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  if (!requireDirectory(TEST262_DIR,
    "cd runtime/external-tests && git clone --depth 1 https://github.com/tc39/test262 test262")) {
    process.exit(1);
  }

  console.log("\nTest262 — ECMAScript Conformance Suite");
  console.log("Engine: Node.js " + process.version + " (V8)\n");

  const allFiles = collectTests(TEST_DIR);
  const files    = Number.isFinite(LIMIT) ? allFiles.slice(0, LIMIT) : allFiles;
  console.log(`Running ${files.length.toLocaleString()} tests (${allFiles.length.toLocaleString()} total, ${allFiles.length - files.length} skipped by --limit)`);

  const results: TestResult[] = [];
  let   done = 0;

  const BATCH = 64; // print progress every N tests
  const start = performance.now();

  for (const f of files) {
    results.push(runOneTest(f));
    done++;
    if (done % BATCH === 0) {
      const pct = ((done / files.length) * 100).toFixed(0);
      const pass = results.filter(r => r.passed).length;
      process.stdout.write(`\r  ${pct}% (${done}/${files.length}) — ${pass} passed`);
    }
  }

  process.stdout.write("\n");
  const elapsed = performance.now() - start;
  const report  = buildReport("ECMAScript — Test262", results, elapsed);

  printSummary({
    ...report,
    results: results.filter(r => !r.passed && !r.skipped).slice(0, 20), // show first 20 failures only
  });
  console.log(`(showing first 20 failures; see report for full list)`);

  await writeReport("test262.json", report);

  if (report.failed > 0) process.exit(1);
}

main().catch((e) => { console.error(e); process.exit(1); });
