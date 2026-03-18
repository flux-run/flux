/**
 * generate-report.ts
 *
 * Reads all JSON reports from runtime/reports/ and generates a Markdown
 * compatibility summary at runtime/reports/summary.md.
 *
 * Usage
 * -----
 *   npm run report          # generate summary from latest reports
 *   npm run test:all        # run all suites then generate summary
 */

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { existsSync }                  from "node:fs";
import { resolve }                     from "node:path";
import { REPORTS_DIR }                 from "./lib/utils.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface LibrarySummary {
  name:     string;
  passed:   number;
  failed:   number;
  total:    number;
  passRate: string;
}

interface Report {
  suite:     string;
  timestamp: string;
  engine:    string;
  passed:    number;
  failed:    number;
  skipped:   number;
  total:     number;
  passRate:  string;
  elapsedMs: number;
  libraries?: LibrarySummary[];  // only in npm-tests.json
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function loadReport(filename: string): Promise<Report | null> {
  const path = resolve(REPORTS_DIR, filename);
  if (!existsSync(path)) return null;
  const raw = await readFile(path, "utf-8");
  return JSON.parse(raw) as Report;
}

function bar(pct: number, width = 20): string {
  const filled = Math.round((pct / 100) * width);
  return "█".repeat(filled) + "░".repeat(width - filled);
}

function statusIcon(passRate: string): string {
  const n = parseFloat(passRate);
  if (isNaN(n)) return "⚪";
  if (n >= 99)  return "🟢";
  if (n >= 90)  return "🟡";
  if (n >= 70)  return "🟠";
  return "🔴";
}

// ---------------------------------------------------------------------------
// Markdown generator
// ---------------------------------------------------------------------------

async function generateSummary(): Promise<string> {
  const now = new Date().toISOString().slice(0, 10);

  const [test262, nodeTests, webTests, npmTests] = await Promise.all([
    loadReport("test262.json"),
    loadReport("node-tests.json"),
    loadReport("web-tests.json"),
    loadReport("npm-tests.json"),
  ]);

  const lines: string[] = [];

  // Header
  lines.push("# Flux Runtime Compatibility Report");
  lines.push("");
  lines.push(`> Generated: ${now}`);
  if (test262) lines.push(`> Engine: ${test262.engine}`);
  lines.push("");

  // Summary table
  lines.push("## Summary");
  lines.push("");
  lines.push("| Suite | Passed | Failed | Skipped | Pass Rate | Status |");
  lines.push("|-------|--------|--------|---------|-----------|--------|");

  for (const [label, r] of [
    ["ECMAScript (Test262)",  test262],
    ["Node.js Core Tests",    nodeTests],
    ["Web Platform Tests",    webTests],
    ["NPM Ecosystem",         npmTests],
  ] as [string, Report | null][]) {
    if (!r) {
      lines.push(`| ${label} | — | — | — | N/A | ⚪ not run |`);
    } else {
      lines.push(`| ${label} | ${r.passed.toLocaleString()} | ${r.failed.toLocaleString()} | ${r.skipped.toLocaleString()} | ${r.passRate} | ${statusIcon(r.passRate)} |`);
    }
  }

  lines.push("");

  // ECMAScript / Test262
  lines.push("---");
  lines.push("");
  lines.push("## ECMAScript — Test262");
  lines.push("");
  if (!test262) {
    lines.push("_Not run. Clone test262 and run `npm run test:262`._");
    lines.push("");
    lines.push("```bash");
    lines.push("cd runtime/external-tests");
    lines.push("git clone --depth 1 https://github.com/tc39/test262 test262");
    lines.push("cd ../runners && npm run test:262");
    lines.push("```");
  } else {
    const pct = parseFloat(test262.passRate);
    lines.push(`**${test262.passRate}** pass rate &nbsp; ${bar(pct)}`);
    lines.push("");
    lines.push(`| Metric | Value |`);
    lines.push(`|--------|-------|`);
    lines.push(`| Passed  | ${test262.passed.toLocaleString()} |`);
    lines.push(`| Failed  | ${test262.failed.toLocaleString()} |`);
    lines.push(`| Skipped | ${test262.skipped.toLocaleString()} (module-flag / browser-only) |`);
    lines.push(`| Total   | ${test262.total.toLocaleString()} |`);
    lines.push(`| Runtime | ${(test262.elapsedMs / 1000).toFixed(0)}s |`);
  }
  lines.push("");

  // Node.js
  lines.push("---");
  lines.push("");
  lines.push("## Node.js Core Tests");
  lines.push("");
  if (!nodeTests) {
    lines.push("_Not run. Copy node/test/parallel and run `npm run test:node`._");
    lines.push("");
    lines.push("```bash");
    lines.push("bash runtime/scripts/setup-external-tests.sh node");
    lines.push("cd runtime/runners && npm run test:node");
    lines.push("```");
  } else {
    const pct = parseFloat(nodeTests.passRate);
    lines.push(`**${nodeTests.passRate}** pass rate &nbsp; ${bar(pct)}`);
    lines.push("");
    lines.push("> Note: Tests requiring native modules, worker_threads, or cluster are auto-skipped.");
    lines.push("");
    lines.push(`| Metric | Value |`);
    lines.push(`|--------|-------|`);
    lines.push(`| Passed  | ${nodeTests.passed.toLocaleString()} |`);
    lines.push(`| Failed  | ${nodeTests.failed.toLocaleString()} |`);
    lines.push(`| Skipped | ${nodeTests.skipped.toLocaleString()} |`);
  }
  lines.push("");

  // Web Platform Tests
  lines.push("---");
  lines.push("");
  lines.push("## Web Platform Tests");
  lines.push("");
  if (!webTests) {
    lines.push("_Not run. Sparse-clone wpt and run `npm run test:web`._");
    lines.push("");
    lines.push("```bash");
    lines.push("bash runtime/scripts/setup-external-tests.sh wpt");
    lines.push("cd runtime/runners && npm run test:web");
    lines.push("```");
  } else {
    const pct = parseFloat(webTests.passRate);
    lines.push(`**${webTests.passRate}** pass rate &nbsp; ${bar(pct)}`);
    lines.push("");
    lines.push("| Scope | Tests |");
    lines.push("|-------|-------|");
    lines.push("| `url/`      | URL & URLSearchParams parsing |");
    lines.push("| `fetch/`    | Fetch API — request, response, headers |");
    lines.push("| `encoding/` | TextEncoder / TextDecoder |");
    lines.push("");
    lines.push("> Tests requiring DOM, ServiceWorker, or browser globals are auto-skipped.");
  }
  lines.push("");

  // NPM Ecosystem
  lines.push("---");
  lines.push("");
  lines.push("## NPM Ecosystem");
  lines.push("");
  if (!npmTests) {
    lines.push("_Not run. Run `npm run test:npm`._");
  } else {
    if (npmTests.libraries) {
      lines.push("| Library | Pass Rate | Status |");
      lines.push("|---------|-----------|--------|");
      for (const lib of npmTests.libraries) {
        const pct = parseFloat(lib.passRate);
        const icon = pct >= 100 ? "✅" : pct >= 80 ? "🟡" : "❌";
        lines.push(`| \`${lib.name}\` | ${lib.passRate} (${lib.passed}/${lib.total}) | ${icon} |`);
      }
    } else {
      lines.push(`**${npmTests.passRate}** pass rate — ${npmTests.passed}/${npmTests.total} tests`);
    }
  }
  lines.push("");

  // Footer
  lines.push("---");
  lines.push("");
  lines.push("_Flux runtime uses Deno V8 isolates. ECMAScript compliance reflects the V8 engine._");
  lines.push("_Node.js API compatibility reflects Deno's Node.js compatibility layer._");
  lines.push("");

  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log("\nGenerating compatibility summary…");

  const md   = await generateSummary();
  const path = resolve(REPORTS_DIR, "summary.md");

  await mkdir(REPORTS_DIR, { recursive: true });
  await writeFile(path, md, "utf-8");

  console.log(`Summary written → ${path}\n`);

  // Print a compact version to stdout
  const compact = md
    .split("\n")
    .filter(l => l.startsWith("##") || l.startsWith("|") || l.startsWith("**"))
    .join("\n");
  console.log(compact);
}

main().catch((e) => { console.error(e); process.exit(1); });
