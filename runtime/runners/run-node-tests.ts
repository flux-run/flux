/**
 * run-node-tests.ts
 *
 * Runs a subset of Node.js `test/parallel` and `test/sequential` tests
 * via `flux run` to measure how much of the Node.js built-in API surface
 * works correctly inside Flux's V8 isolates.
 *
 * Prerequisites
 * -------------
 * Copy only the test directories from the Node.js repo:
 *   bash runtime/scripts/setup-external-tests.sh node
 *
 * Usage
 * -----
 *   npm run test:node
 *   npm run test:node -- --filter fs   # only tests whose filename contains "fs"
 *   npm run test:node -- --limit 100
 *
 * Output
 * ------
 *   runtime/reports/node-tests.json
 */

import { spawnSync }          from "node:child_process";
import { readdirSync, statSync } from "node:fs";
import { join, resolve, basename } from "node:path";
import { FLUX_CLI_BIN }       from "./lib/flux-binary.js";
import { performance }        from "node:perf_hooks";
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

const NODE_CORE_DIR = resolve(EXTERNAL_TESTS_DIR, "node-core");

/** Test filename patterns that are inherently Node-only and always skipped. */
const SKIP_PATTERNS = [
  /native/i,
  /worker_threads/i,
  /cluster/i,
  /repl/i,
  /inspector/i,
  /debugger/i,
  /addon/i,
  /gyp/i,
  /napi/i,
  /binding/i,
  /dlopen/i,
];

const filterArg = process.argv.indexOf("--filter");
const FILTER    = filterArg !== -1 ? process.argv[filterArg + 1] : undefined;

const limitArg = process.argv.indexOf("--limit");
const LIMIT    = limitArg !== -1 ? parseInt(process.argv[limitArg + 1] ?? "9999", 10) : Infinity;

// ---------------------------------------------------------------------------
// Test file discovery
// ---------------------------------------------------------------------------

function collectJsFiles(dir: string): string[] {
  if (!isDir(dir)) return [];
  return readdirSync(dir)
    .filter(f => f.endsWith(".js") || f.endsWith(".mjs"))
    .map(f => join(dir, f));
}

function isDir(p: string): boolean {
  try { return statSync(p).isDirectory(); } catch { return false; }
}

// ---------------------------------------------------------------------------
// Per-test runner
// ---------------------------------------------------------------------------

function shouldSkip(filename: string): boolean {
  return SKIP_PATTERNS.some(re => re.test(filename));
}

function runOneTest(filePath: string, timeoutMs = 10_000): TestResult {
  const name = basename(filePath);
  const t0   = performance.now();

  if (shouldSkip(name)) {
    return { name, passed: false, skipped: true, duration: 0 };
  }
  if (FILTER && !name.includes(FILTER)) {
    return { name, passed: false, skipped: true, duration: 0 };
  }

  const result = spawnSync(
    FLUX_CLI_BIN,
    ["run", filePath],
    { timeout: timeoutMs, encoding: "utf-8" },
  );

  const passed = result.status === 0 && !result.error;
  return {
    name,
    passed,
    skipped: false,
    error: passed ? undefined : ((result.stderr || result.error?.message || "non-zero exit").slice(0, 300)),
    duration: Math.round(performance.now() - t0),
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  if (!requireDirectory(NODE_CORE_DIR,
    "bash runtime/scripts/setup-external-tests.sh node")) {
    process.exit(1);
  }

  const parallelDir   = join(NODE_CORE_DIR, "parallel");
  const sequentialDir = join(NODE_CORE_DIR, "sequential");

  const parallelFiles   = collectJsFiles(parallelDir);
  const sequentialFiles = collectJsFiles(sequentialDir);
  let allFiles          = [...parallelFiles, ...sequentialFiles];

  if (Number.isFinite(LIMIT)) allFiles = allFiles.slice(0, LIMIT);

  console.log("\nNode.js Core Compatibility Tests");
  console.log(`Engine: ${process.version}`);
  console.log(`Tests:  ${allFiles.length} (parallel: ${parallelFiles.length}, sequential: ${sequentialFiles.length})\n`);

  const results: TestResult[] = [];
  const start   = performance.now();
  const BATCH   = 20;

  for (const f of allFiles) {
    results.push(runOneTest(f));
    if (results.length % BATCH === 0) {
      const pass = results.filter(r => r.passed).length;
      const pct  = ((results.length / allFiles.length) * 100).toFixed(0);
      process.stdout.write(`\r  ${pct}% (${results.length}/${allFiles.length}) — ${pass} passed`);
    }
  }

  process.stdout.write("\n");
  const report = buildReport("Node.js Core Tests", results, performance.now() - start);

  // Print only failures for brevity
  printSummary({
    ...report,
    results: results.filter(r => !r.passed && !r.skipped).slice(0, 30),
  });

  await writeReport("node-tests.json", report);
  if (report.failed > 0) process.exit(1);
}

main().catch((e) => { console.error(e); process.exit(1); });
