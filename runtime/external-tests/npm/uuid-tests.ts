/**
 * uuid — compatibility tests
 */

import { v1, v4, v5, v3, validate, version, parse, stringify, NIL } from "uuid";
import type { TestResult } from "../../runners/lib/utils.js";

function run(name: string, fn: () => void): TestResult {
  const t0 = performance.now();
  try {
    fn();
    return { name, passed: true, skipped: false, duration: Math.round(performance.now() - t0) };
  } catch (e) {
    return {
      name, passed: false, skipped: false,
      error: e instanceof Error ? e.message : String(e),
      duration: Math.round(performance.now() - t0),
    };
  }
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MY_NAMESPACE = "6ba7b810-9dad-11d1-80b4-00c04fd430c8"; // standard DNS namespace

export async function runUuidTests(): Promise<TestResult[]> {
  return [
    run("v4 generates a valid RFC 4122 UUID", () => {
      const id = v4();
      if (!UUID_RE.test(id)) throw new Error(`invalid UUID: ${id}`);
    }),

    run("v4 generates a different UUID each call", () => {
      const ids = Array.from({ length: 100 }, () => v4());
      const set = new Set(ids);
      if (set.size !== 100) throw new Error("collision detected");
    }),

    run("v4 version field is 4", () => {
      const id = v4();
      if (version(id) !== 4) throw new Error(`version: ${version(id)}`);
    }),

    run("v1 generates a valid time-based UUID", () => {
      const id = v1();
      if (!UUID_RE.test(id)) throw new Error(`invalid UUID: ${id}`);
      if (version(id) !== 1) throw new Error(`version: ${version(id)}`);
    }),

    run("v3 is deterministic for same namespace + name", () => {
      const a = v3("hello", MY_NAMESPACE);
      const b = v3("hello", MY_NAMESPACE);
      if (a !== b) throw new Error("v3 not deterministic");
      if (version(a) !== 3) throw new Error(`version: ${version(a)}`);
    }),

    run("v5 is deterministic for same namespace + name", () => {
      const a = v5("hello", MY_NAMESPACE);
      const b = v5("hello", MY_NAMESPACE);
      if (a !== b) throw new Error("v5 not deterministic");
      if (version(a) !== 5) throw new Error(`version: ${version(a)}`);
    }),

    run("v3 and v5 differ for same input (different algorithms)", () => {
      const a = v3("hello", MY_NAMESPACE);
      const b = v5("hello", MY_NAMESPACE);
      if (a === b) throw new Error("v3 and v5 should differ");
    }),

    run("validate returns true for a valid UUID", () => {
      if (!validate(v4())) throw new Error("valid UUID failed validate");
    }),

    run("validate returns false for garbage string", () => {
      if (validate("not-a-uuid")) throw new Error("garbage passed validate");
      if (validate(""))          throw new Error("empty string passed validate");
    }),

    run("parse + stringify round-trips correctly", () => {
      const original = v4();
      const bytes    = parse(original);
      const restored = stringify(bytes);
      if (original.toLowerCase() !== restored.toLowerCase()) {
        throw new Error(`round-trip mismatch: ${original} → ${restored}`);
      }
    }),

    run("NIL UUID is all zeros", () => {
      if (NIL !== "00000000-0000-0000-0000-000000000000") throw new Error("NIL wrong");
      if (!validate(NIL)) throw new Error("NIL not valid");
    }),
  ];
}
