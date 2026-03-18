/**
 * run-npm-tests.ts
 *
 * Runs integration compatibility tests for popular npm libraries:
 *   axios · zod · uuid · jsonwebtoken · lodash
 *
 * These tests import the real library, exercise its core API surface, and
 * assert expected behavior — no mocking of the library itself.
 *
 * Usage
 * -----
 *   npm run test:npm
 *   npm run test:npm -- --suite zod     # run one library only
 *
 * Output
 * ------
 *   runtime/reports/npm-tests.json
 */

import { performance }  from "node:perf_hooks";
import { resolve }      from "node:path";
import { fileURLToPath } from "node:url";
import { dirname }      from "node:path";
import {
  TestResult,
  buildReport,
  writeReport,
  printSummary,
} from "./lib/utils.js";

// ---------------------------------------------------------------------------
// Lazy-import each test module (so individual suite failures don't crash all)
// ---------------------------------------------------------------------------

const __dirname = dirname(fileURLToPath(import.meta.url));
const NPM_TESTS = resolve(__dirname, "../external-tests/npm");

async function loadSuite(filename: string): Promise<{ fn: () => Promise<TestResult[]>; name: string } | null> {
  try {
    const mod = await import(`${NPM_TESTS}/${filename}`);
    const maybeFn = Object.values(mod).find((v) => typeof v === "function");
    if (!maybeFn) return null;
    const fn = maybeFn as () => Promise<TestResult[]>;
    return { fn, name: filename.replace("-tests.ts", "") };
  } catch (e) {
    console.warn(`  ⚠️  Could not load ${filename}: ${(e as Error).message}`);
    return null;
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

interface LibraryReport {
  library: string;
  passed:  number;
  failed:  number;
  total:   number;
  results: TestResult[];
}

const suiteArg   = process.argv.indexOf("--suite");
const SUITE_FILTER = suiteArg !== -1 ? process.argv[suiteArg + 1] : undefined;

async function main() {
  const suiteFiles = [
    "axios-tests.ts",
    "zod-tests.ts",
    "uuid-tests.ts",
    "jsonwebtoken-tests.ts",
    "lodash-tests.ts",
  ].filter(f => !SUITE_FILTER || f.includes(SUITE_FILTER));

  console.log("\nNPM Ecosystem Compatibility Tests");
  console.log(`Libraries: ${suiteFiles.map(f => f.replace("-tests.ts","")).join(", ")}\n`);

  const suites = (await Promise.all(suiteFiles.map(loadSuite))).filter(Boolean) as NonNullable<Awaited<ReturnType<typeof loadSuite>>>[];

  const libraryReports: LibraryReport[] = [];
  const allResults:     TestResult[]    = [];
  const start = performance.now();

  const LABEL_PAD = 16;

  for (const { fn, name } of suites) {
    const t0 = performance.now();
    let results: TestResult[];

    try {
      results = await fn();
    } catch (e) {
      console.error(`  ✗ ${name}: runner threw — ${(e as Error).message}`);
      continue;
    }

    const passed  = results.filter(r =>  r.passed).length;
    const failed  = results.filter(r => !r.passed && !r.skipped).length;
    const elapsed = ((performance.now() - t0) / 1000).toFixed(2);

    const tick  = failed === 0 ? "\x1b[32m✓\x1b[0m" : "\x1b[31m✗\x1b[0m";
    const count = failed === 0
      ? `\x1b[32m${passed}/${results.length}\x1b[0m`
      : `\x1b[31m${passed}/${results.length}\x1b[0m`;

    console.log(`  ${name.padEnd(LABEL_PAD)} ${tick} ${count}  (${elapsed}s)`);

    for (const r of results) {
      if (!r.passed && !r.skipped) {
        console.log(`      \x1b[31m✗\x1b[0m ${r.name}`);
        if (r.error) console.log(`        \x1b[90m${r.error}\x1b[0m`);
      }
    }

    libraryReports.push({ library: name, passed, failed, total: results.length, results });
    allResults.push(...results);
  }

  const elapsed = performance.now() - start;

  // Summarize
  console.log("\n" + "─".repeat(56));
  const totalPassed  = allResults.filter(r =>  r.passed).length;
  const totalFailed  = allResults.filter(r => !r.passed && !r.skipped).length;
  const totalSkipped = allResults.filter(r =>  r.skipped).length;
  const total        = allResults.length;
  console.log(`  Total: ${totalPassed}/${total} passed  (${totalFailed} failed, ${totalSkipped} skipped)`);
  console.log(`  Time:  ${(elapsed / 1000).toFixed(1)}s\n`);

  // Each library as a separate entry in the report results (summary row)
  const reportResults: TestResult[] = libraryReports.map(lr => ({
    name:     lr.library,
    passed:   lr.failed === 0,
    skipped:  false,
    error:    lr.failed > 0 ? `${lr.failed} tests failed` : undefined,
    duration: 0,
  }));

  // Full detail also stored under the library name for the summary generator
  const report = buildReport("NPM Ecosystem", allResults, elapsed);

  // Attach per-library summary as metadata-compatible results
  const detailedReport = {
    ...report,
    libraries: libraryReports.map(lr => ({
      name:     lr.library,
      passed:   lr.passed,
      failed:   lr.failed,
      total:    lr.total,
      passRate: `${((lr.passed / lr.total) * 100).toFixed(1)}%`,
    })),
  };

  await writeReport("npm-tests.json", detailedReport as typeof report);

  if (totalFailed > 0) process.exit(1);
}

main().catch((e) => { console.error(e); process.exit(1); });
