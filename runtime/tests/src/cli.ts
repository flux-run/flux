#!/usr/bin/env node

import { createECMAScriptSuite } from "../suites/ecmascript/index.js";
import { createNodeSuite } from "../suites/node/index.js";
import { createWebSuite } from "../suites/web/index.js";
import { createFrameworkSuite } from "../suites/frameworks/index.js";
import { createRuntimeSuite } from "../suites/runtime/index.js";
import { createDeterminismSuite } from "../suites/determinism/index.js";
import { createErrorHandlingSuite } from "../suites/error-handling/index.js";
import { createConcurrencySuite } from "../suites/concurrency/index.js";
import { createTrustSuites } from "../suites/trust/index.js";
import { createCompatSuites } from "../suites/compatibility/index.js";
import { createReplaySuite } from "../suites/replay/index.js";
import { createModuleLoaderSuite } from "../suites/module-loader/index.js";
import { createTestSummary, formatTestResults } from "./utils.js";
import chalk from "chalk";

const args = process.argv.slice(2);
const filter = args[0] || "all";

// ---------------------------------------------------------------------------
// Trust Test Suite runner — compact dashboard output (flux runtime:test)
// ---------------------------------------------------------------------------
async function runTrust() {
  const { performance } = await import("perf_hooks");
  const start = performance.now();

  console.log(chalk.bold.white("\nFlux Runtime Test Suite\n"));

  const categories = createTrustSuites();
  let totalPassed = 0;
  let totalTests = 0;
  const failures: string[] = [];

  const PAD = 16; // column width for category label

  for (const { label, suite } of categories) {
    const result = await suite.run();
    totalPassed += result.passed;
    totalTests  += result.total;

    const ok = result.failed === 0;
    const tick = ok ? chalk.green("✓") : chalk.red("✗");
    const count = ok
      ? chalk.green(`${result.passed}/${result.total}`)
      : chalk.red(`${result.passed}/${result.total}`);

    console.log(`  ${label.padEnd(PAD)} ${tick} ${count}`);

    if (!ok) {
      for (const t of result.tests) {
        if (!t.passed) {
          failures.push(`  ${chalk.red("✗")} [${label}] ${t.name}`);
          if (t.error) failures.push(`      ${chalk.gray(t.error)}`);
        }
      }
    }
  }

  const elapsed = ((performance.now() - start) / 1000).toFixed(1);
  const totalFailed = totalTests - totalPassed;

  console.log("");
  console.log(`  Total: ${totalPassed}/${totalTests} passed`);
  console.log(`  Time:  ${elapsed}s`);
  console.log("");

  if (failures.length > 0) {
    console.log(chalk.red.bold("Failures:\n"));
    failures.forEach((l) => console.log(l));
    console.log("");
    console.log(chalk.red.bold(`❌ ${totalFailed} ${totalFailed === 1 ? "test" : "tests"} failed\n`));
    process.exit(1);
  } else {
    console.log(chalk.green.bold(`✅ All ${totalTests} tests passed\n`));
    process.exit(0);
  }
}

// ---------------------------------------------------------------------------
// Generic single-suite runner (used for replay, modules, etc.)
// ---------------------------------------------------------------------------
async function runSingleSuite(title: string, suite: ReturnType<typeof createReplaySuite>) {
  const { performance } = await import("perf_hooks");
  const start = performance.now();

  const result = await suite.run();
  const elapsed = ((performance.now() - start) / 1000).toFixed(1);

  console.log(chalk.bold.white(`\n${title}\n`));
  for (const t of result.tests) {
    const tick = t.passed ? chalk.green("✓") : chalk.red("✗");
    console.log(`  ${tick} ${t.name}`);
    if (!t.passed && t.error) console.log(`      ${chalk.gray(t.error)}`);
  }
  console.log("");
  console.log(`  Total: ${result.passed}/${result.total} passed`);
  console.log(`  Time:  ${elapsed}s\n`);

  if (result.failed > 0) {
    console.log(chalk.red.bold(`❌ ${result.failed} ${result.failed === 1 ? "test" : "tests"} failed\n`));
    process.exit(1);
  } else {
    console.log(chalk.green.bold(`✅ All ${result.total} tests passed\n`));
    process.exit(0);
  }
}

// ---------------------------------------------------------------------------
// Compatibility Suite runner — same compact dashboard format
// ---------------------------------------------------------------------------
async function runCompat() {
  const { performance } = await import("perf_hooks");
  const start = performance.now();

  console.log(chalk.bold.white("\nFlux Compatibility Suite\n"));

  const categories = createCompatSuites();
  let totalPassed = 0;
  let totalTests  = 0;
  const failures: string[] = [];

  const PAD = 18;

  for (const { label, suite } of categories) {
    const result = await suite.run();
    totalPassed += result.passed;
    totalTests  += result.total;

    const ok    = result.failed === 0;
    const tick  = ok ? chalk.green("✓") : chalk.red("✗");
    const count = ok
      ? chalk.green(`${result.passed}/${result.total}`)
      : chalk.red(`${result.passed}/${result.total}`);

    console.log(`  ${label.padEnd(PAD)} ${tick} ${count}`);

    if (!ok) {
      for (const t of result.tests) {
        if (!t.passed) {
          failures.push(`  ${chalk.red("✗")} [${label}] ${t.name}`);
          if (t.error) failures.push(`      ${chalk.gray(t.error)}`);
        }
      }
    }
  }

  const elapsed    = ((performance.now() - start) / 1000).toFixed(1);
  const totalFailed = totalTests - totalPassed;

  console.log("");
  console.log(`  Total: ${totalPassed}/${totalTests} passed`);
  console.log(`  Time:  ${elapsed}s`);
  console.log("");

  if (failures.length > 0) {
    console.log(chalk.red.bold("Failures:\n"));
    failures.forEach((l) => console.log(l));
    console.log("");
    console.log(chalk.red.bold(`❌ ${totalFailed} ${totalFailed === 1 ? "test" : "tests"} failed\n`));
    process.exit(1);
  } else {
    console.log(chalk.green.bold(`✅ All ${totalTests} tests passed\n`));
    process.exit(0);
  }
}

async function main() {
  // `flux runtime:test` or `node cli.js trust` → compact trust dashboard
  if (filter === "trust") {
    await runTrust();
    return;
  }

  // `node cli.js compat` → compatibility suite dashboard
  if (filter === "compat") {
    await runCompat();
    return;
  }

  // `node cli.js replay` → dedicated replay end-to-end suite
  if (filter === "replay") {
    await runSingleSuite("Replay (end-to-end)", createReplaySuite());
    return;
  }

  // `node cli.js modules` → module loader suite
  if (filter === "modules") {
    await runSingleSuite("Module Loader", createModuleLoaderSuite());
    return;
  }

  console.log(chalk.bold.blue("\n🚀 Flux Runtime Test Suite\n"));

  const suites = {
    ecmascript: createECMAScriptSuite(),
    node: createNodeSuite(),
    web: createWebSuite(),
    frameworks: createFrameworkSuite(),
    runtime: createRuntimeSuite(),
    determinism: createDeterminismSuite(),
    "error-handling": createErrorHandlingSuite(),
    concurrency: createConcurrencySuite(),
  };

  const toRun =
    filter === "all"
      ? Object.entries(suites)
      : Object.entries(suites).filter(([name]) => name.includes(filter));

  if (toRun.length === 0) {
    console.log(chalk.yellow(`\n⚠️  No suites matching filter: "${filter}"`));
    console.log(chalk.gray("Available suites:"));
    Object.keys(suites).forEach((name) => {
      console.log(chalk.gray(`  - ${name}`));
    });
    console.log(chalk.gray("  - trust   (compact dashboard — 40 high-signal tests)"));
    console.log(chalk.gray("  - compat  (compatibility suite — real app patterns)"));
    console.log(chalk.gray("  - replay  (end-to-end record → replay → compare tests)"));
    console.log(chalk.gray("  - modules (module loader: ESM, dynamic, circular, cache)"));
    process.exit(1);
  }

  const results = [];
  for (const [name, suite] of toRun) {
    console.log(chalk.cyan(`Running ${name}...`));
    const result = await suite.run();
    results.push(result);
    console.log(
      chalk[result.failed === 0 ? "green" : "red"](`  ${result.passed}/${result.total} passed`)
    );
  }

  const summary = createTestSummary(results);
  console.log(formatTestResults(summary));

  if (summary.totalFailed > 0) {
    console.log(chalk.red.bold(`\n❌ ${summary.totalFailed} tests failed\n`));
    process.exit(1);
  } else {
    console.log(chalk.green.bold(`\n✅ All ${summary.totalTests} tests passed!\n`));
    process.exit(0);
  }
}

main().catch((err) => {
  console.error(chalk.red("Fatal error:"), err);
  process.exit(1);
});
