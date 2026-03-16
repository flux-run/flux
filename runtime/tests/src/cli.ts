#!/usr/bin/env node

import { createECMAScriptSuite } from "../suites/ecmascript/index.js";
import { createNodeSuite } from "../suites/node/index.js";
import { createWebSuite } from "../suites/web/index.js";
import { createFrameworkSuite } from "../suites/frameworks/index.js";
import { createRuntimeSuite } from "../suites/runtime/index.js";
import { createDeterminismSuite } from "../suites/determinism/index.js";
import { createErrorHandlingSuite } from "../suites/error-handling/index.js";
import { createConcurrencySuite } from "../suites/concurrency/index.js";
import { createTestSummary, formatTestResults } from "./utils.js";
import chalk from "chalk";

const args = process.argv.slice(2);
const filter = args[0] || "all";

async function main() {
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
