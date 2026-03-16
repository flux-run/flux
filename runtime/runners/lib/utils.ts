/**
 * Shared types and helpers used by all compatibility runners.
 */

import { writeFile, mkdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export interface TestResult {
  name:     string;
  passed:   boolean;
  skipped:  boolean;
  error?:   string;
  duration: number; // ms
}

export interface SuiteReport {
  suite:      string;
  timestamp:  string;
  engine:     string;       // e.g. "node v22.x.x"
  passed:     number;
  failed:     number;
  skipped:    number;
  total:      number;
  elapsedMs:  number;
  passRate:   string;       // e.g. "98.6%"
  results:    TestResult[];
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

/** Absolute path to `runtime/` (two levels up from runners/lib/). */
export const RUNTIME_DIR        = resolve(__dirname, "../..");
export const REPORTS_DIR        = resolve(RUNTIME_DIR, "reports");
export const EXTERNAL_TESTS_DIR = resolve(RUNTIME_DIR, "external-tests");

// ---------------------------------------------------------------------------
// Report writing
// ---------------------------------------------------------------------------

export async function writeReport(filename: string, report: SuiteReport): Promise<void> {
  await mkdir(REPORTS_DIR, { recursive: true });
  const path = resolve(REPORTS_DIR, filename);
  await writeFile(path, JSON.stringify(report, null, 2), "utf-8");
  console.log(`\nReport saved → ${path}`);
}

export async function readReport(filename: string): Promise<SuiteReport | null> {
  const path = resolve(REPORTS_DIR, filename);
  if (!existsSync(path)) return null;
  const raw = await readFile(path, "utf-8");
  return JSON.parse(raw) as SuiteReport;
}

// ---------------------------------------------------------------------------
// Report builder
// ---------------------------------------------------------------------------

export function buildReport(
  suite: string,
  results: TestResult[],
  elapsedMs: number,
): SuiteReport {
  const passed  = results.filter(r => r.passed && !r.skipped).length;
  const failed  = results.filter(r => !r.passed && !r.skipped).length;
  const skipped = results.filter(r => r.skipped).length;
  const total   = results.length;
  const passRate = total - skipped === 0
    ? "N/A"
    : `${((passed / (total - skipped)) * 100).toFixed(1)}%`;

  return {
    suite,
    timestamp: new Date().toISOString(),
    engine:    `node ${process.version}`,
    passed,
    failed,
    skipped,
    total,
    elapsedMs: Math.round(elapsedMs),
    passRate,
    results,
  };
}

// ---------------------------------------------------------------------------
// Printer
// ---------------------------------------------------------------------------

const green  = (s: string) => `\x1b[32m${s}\x1b[0m`;
const red    = (s: string) => `\x1b[31m${s}\x1b[0m`;
const yellow = (s: string) => `\x1b[33m${s}\x1b[0m`;
const gray   = (s: string) => `\x1b[90m${s}\x1b[0m`;
const bold   = (s: string) => `\x1b[1m${s}\x1b[0m`;

export function printSummary(report: SuiteReport): void {
  const line = "─".repeat(56);
  console.log(`\n${bold(report.suite)}`);
  console.log(line);

  for (const r of report.results) {
    if (r.skipped) {
      console.log(`  ${yellow("○")} ${gray(r.name)}`);
    } else if (r.passed) {
      console.log(`  ${green("✓")} ${r.name}`);
    } else {
      console.log(`  ${red("✗")} ${r.name}`);
      if (r.error) console.log(`      ${gray(r.error)}`);
    }
  }

  console.log(line);
  console.log(`  Passed:  ${green(String(report.passed))}`);
  console.log(`  Failed:  ${report.failed > 0 ? red(String(report.failed)) : green("0")}`);
  console.log(`  Skipped: ${yellow(String(report.skipped))}`);
  console.log(`  Total:   ${report.total}`);
  console.log(`  Rate:    ${report.passRate}`);
  console.log(`  Time:    ${(report.elapsedMs / 1000).toFixed(1)}s\n`);
}

// ---------------------------------------------------------------------------
// Timeout runner
// ---------------------------------------------------------------------------

export async function runWithTimeout<T>(
  fn: () => Promise<T>,
  timeoutMs = 5000,
): Promise<T> {
  return Promise.race([
    fn(),
    new Promise<T>((_, reject) =>
      setTimeout(() => reject(new Error(`timed out after ${timeoutMs}ms`)), timeoutMs),
    ),
  ]);
}

// ---------------------------------------------------------------------------
// Prerequisite check
// ---------------------------------------------------------------------------

export function requireDirectory(path: string, setupHint: string): boolean {
  if (!existsSync(path)) {
    console.error(`\n⚠️  External tests directory not found:\n   ${path}`);
    console.error(`\n   ${setupHint}\n`);
    return false;
  }
  return true;
}
