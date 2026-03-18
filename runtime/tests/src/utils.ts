// Test utility helpers
import { TestHarness, TestResult, SuiteResult } from "./harness.js";

export function createSuite(name: string): TestHarness {
  return new TestHarness(name);
}

export function formatMs(ms: number): string {
  return `${ms.toFixed(2)}ms`;
}

export function timeout(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function randomBytes(length: number): Buffer {
  return Buffer.from(Array.from({ length }, () => Math.floor(Math.random() * 256)));
}

export function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function withTimeout<T>(
  promise: Promise<T>,
  ms: number,
  message = "Operation timed out"
): Promise<T> {
  const timeoutPromise = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error(message)), ms)
  );
  return Promise.race([promise, timeoutPromise]);
}

export function createTestSummary(results: SuiteResult[]): {
  totalTests: number;
  totalPassed: number;
  totalFailed: number;
  totalDuration: number;
  suites: SuiteResult[];
} {
  return {
    totalTests: results.reduce((sum, r) => sum + r.total, 0),
    totalPassed: results.reduce((sum, r) => sum + r.passed, 0),
    totalFailed: results.reduce((sum, r) => sum + r.failed, 0),
    totalDuration: results.reduce((sum, r) => sum + r.duration, 0),
    suites: results,
  };
}

export function formatTestResults(summary: {
  totalTests: number;
  totalPassed: number;
  totalFailed: number;
  totalDuration: number;
  suites: SuiteResult[];
}): string {
  let output = "\n";
  output += "═".repeat(60) + "\n";
  output += "TEST RESULTS\n";
  output += "═".repeat(60) + "\n\n";

  for (const suite of summary.suites) {
    const status = suite.failed === 0 ? "✓" : "✗";
    output += `${status} ${suite.name} (${suite.passed}/${suite.total}) ${formatMs(suite.duration)}\n`;

    if (suite.failed > 0) {
      for (const test of suite.tests) {
        if (!test.passed) {
          output += `  ✗ ${test.name}\n`;
          if (test.error) {
            output += `    Error: ${test.error}\n`;
          }
        }
      }
    }
  }

  output += "\n" + "─".repeat(60) + "\n";
  output += `Total: ${summary.totalPassed}/${summary.totalTests} passed `;
  output += `(${summary.totalFailed} failed) in ${formatMs(summary.totalDuration)}\n`;
  output += "═".repeat(60) + "\n";

  return output;
}
