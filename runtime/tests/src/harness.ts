// Test harness framework with zora-like API
import { performance } from "perf_hooks";

export interface TestResult {
  name: string;
  passed: boolean;
  duration: number;
  error?: string;
}

export interface SuiteResult {
  name: string;
  tests: TestResult[];
  passed: number;
  failed: number;
  total: number;
  duration: number;
}

export class TestHarness {
  private tests: Array<{ name: string; fn: () => Promise<void> | void }> = [];
  private results: TestResult[] = [];
  public name: string;

  constructor(name: string) {
    this.name = name;
  }

  test(name: string, fn: () => Promise<void> | void) {
    this.tests.push({ name, fn });
  }

  async run(): Promise<SuiteResult> {
    const startTime = performance.now();
    this.results = [];

    for (const { name, fn } of this.tests) {
      const testStart = performance.now();
      let passed = false;
      let error: string | undefined;

      try {
        await Promise.resolve(fn());
        passed = true;
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }

      const duration = performance.now() - testStart;
      this.results.push({ name, passed, duration, error });
    }

    const duration = performance.now() - startTime;
    const passed = this.results.filter((r) => r.passed).length;
    const failed = this.results.filter((r) => !r.passed).length;

    return {
      name: this.name,
      tests: this.results,
      passed,
      failed,
      total: this.results.length,
      duration,
    };
  }
}

export function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(`Assertion failed: ${message}`);
  }
}

export function assertEquals<T>(actual: T, expected: T, message?: string) {
  if (actual !== expected) {
    throw new Error(
      message ||
        `Expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`
    );
  }
}

export function assertThrows(fn: () => void, message?: string) {
  try {
    fn();
    throw new Error(message || "Expected function to throw");
  } catch (e) {
    // Expected
  }
}

export function assertArrayIncludes<T>(array: T[], item: T, message?: string) {
  if (!array.includes(item)) {
    throw new Error(message || `Array does not include ${item}`);
  }
}

export function assertStringIncludes(str: string, include: string, message?: string) {
  if (!str.includes(include)) {
    throw new Error(message || `String does not include "${include}"`);
  }
}
